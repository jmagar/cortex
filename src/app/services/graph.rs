use super::graph_limits::{ExplainPath, GraphExplainLimits, GraphLimits, GraphRowsModels};
use super::graph_safety::*;
use super::graph_support::*;
use super::*;

impl CortexService {
    pub async fn graph_entity_lookup(
        &self,
        req: GraphEntityLookupRequest,
    ) -> ServiceResult<GraphEntityLookupResponse> {
        let limits = GraphLimits::from_entity_request(&req);
        let status = self
            .run_db("graph.status", db::graph::graph_projection_status)
            .await?;

        if let (Some(entity_type), Some(key)) = (req.entity_type.clone(), req.key.clone()) {
            validate_graph_entity_type(&entity_type)?;
            let entity = self
                .run_db("graph.entity_key", move |pool| {
                    db::graph::find_graph_entity_by_key(pool, &entity_type, &key)
                })
                .await?
                .ok_or_else(|| ServiceError::NotFound("graph entity not found".into()))?;
            return Ok(GraphEntityLookupResponse {
                resolved_entity: Some(entity.into()),
                candidates: Vec::new(),
                metadata: graph_metadata(&status, limits, false, None),
            });
        }

        let alias_type = req
            .alias_type
            .clone()
            .ok_or_else(|| ServiceError::InvalidInput("alias_type is required".into()))?;
        let alias_key = req
            .alias_key
            .clone()
            .ok_or_else(|| ServiceError::InvalidInput("alias_key is required".into()))?;
        if alias_type.trim().is_empty() || alias_key.trim().is_empty() {
            return Err(ServiceError::InvalidInput(
                "alias_type and alias_key must be non-empty".into(),
            ));
        }
        let candidates = self
            .run_db("graph.entity_alias", move |pool| {
                db::graph::find_graph_entities_by_alias(pool, &alias_type, &alias_key, limits.limit)
            })
            .await?;
        if candidates.is_empty() {
            return Err(ServiceError::NotFound(
                "graph entity alias not found".into(),
            ));
        }
        let candidates: Vec<GraphEntityCandidate> =
            candidates.into_iter().map(Into::into).collect();
        let resolved_entity = if candidates.len() == 1 {
            candidates.first().map(|candidate| candidate.entity.clone())
        } else {
            None
        };
        Ok(GraphEntityLookupResponse {
            resolved_entity,
            candidates,
            metadata: graph_metadata(&status, limits, false, None),
        })
    }

    pub async fn graph_around(
        &self,
        req: GraphAroundRequest,
    ) -> ServiceResult<GraphAroundResponse> {
        let limits = GraphLimits::from_around_request(&req)?;
        let status = self
            .run_db("graph.status", db::graph::graph_projection_status)
            .await?;

        let (resolved_entity, candidates) = if let Some(entity_id) = req.entity_id {
            let entity = self
                .run_db("graph.entity_id", move |pool| {
                    db::graph::find_graph_entity_by_id(pool, entity_id)
                })
                .await?
                .ok_or_else(|| ServiceError::NotFound("graph entity not found".into()))?;
            (Some(GraphEntity::from(entity)), Vec::new())
        } else {
            let lookup = self
                .graph_entity_lookup(GraphEntityLookupRequest {
                    mode: None,
                    entity_type: req.entity_type.clone(),
                    key: req.key.clone(),
                    alias_type: req.alias_type.clone(),
                    alias_key: req.alias_key.clone(),
                    limit: Some(limits.limit),
                    evidence_sample_limit: Some(limits.evidence_sample_limit),
                    payload_budget: Some(limits.payload_budget),
                })
                .await?;
            (lookup.resolved_entity, lookup.candidates)
        };

        let Some(entity) = resolved_entity.clone() else {
            return Ok(GraphAroundResponse {
                resolved_entity: None,
                entities: Vec::new(),
                relationships: Vec::new(),
                evidence: Vec::new(),
                next_queries: Vec::new(),
                candidates,
                metadata: graph_metadata(
                    &status,
                    limits,
                    true,
                    Some("ambiguous_entity".to_string()),
                ),
            });
        };

        let entity_id = entity.id;
        let rows = self
            .run_db("graph.around", move |pool| {
                db::graph::graph_around_entity(
                    pool,
                    entity_id,
                    limits.limit,
                    limits.evidence_sample_limit,
                )
            })
            .await?;
        let rows_truncated = rows.truncated;
        let converted = graph_rows_to_models(rows, limits.payload_budget);
        let GraphRowsModels {
            relationships,
            entities,
            evidence,
        } = converted;
        let next_queries = entities
            .iter()
            .filter(|related| related.id != entity.id)
            .take(10)
            .map(|related| GraphNextQuery {
                mode: "around".to_string(),
                entity_id: related.id,
                label: related.display_label.clone(),
            })
            .collect();
        let payload_truncated = estimated_graph_payload_bytes(&entities, &relationships, &evidence)
            > limits.payload_budget as usize;
        Ok(GraphAroundResponse {
            resolved_entity: Some(entity),
            entities,
            relationships,
            evidence,
            next_queries,
            candidates: Vec::new(),
            metadata: graph_metadata(
                &status,
                limits,
                rows_truncated || payload_truncated,
                if payload_truncated {
                    Some("payload_budget".to_string())
                } else if rows_truncated {
                    Some("relationship_limit".to_string())
                } else {
                    None
                },
            ),
        })
    }

    pub async fn graph_explain(
        &self,
        req: GraphExplainRequest,
    ) -> ServiceResult<GraphExplainResponse> {
        let limits = GraphExplainLimits::from_request(&req);
        let status = self
            .run_db("graph.status", db::graph::graph_projection_status)
            .await?;

        let (resolved_entity, candidates) = if let Some(entity_id) = req.entity_id {
            let entity = self
                .run_db("graph.entity_id", move |pool| {
                    db::graph::find_graph_entity_by_id(pool, entity_id)
                })
                .await?
                .ok_or_else(|| ServiceError::NotFound("graph entity not found".into()))?;
            (Some(GraphEntity::from(entity)), Vec::new())
        } else {
            let lookup = self
                .graph_entity_lookup(GraphEntityLookupRequest {
                    mode: None,
                    entity_type: req.entity_type.clone(),
                    key: req.key.clone(),
                    alias_type: req.alias_type.clone(),
                    alias_key: req.alias_key.clone(),
                    limit: Some(limits.beam_width),
                    evidence_sample_limit: Some(limits.evidence_sample_limit),
                    payload_budget: Some(limits.payload_budget),
                })
                .await?;
            (lookup.resolved_entity, lookup.candidates)
        };

        let Some(root) = resolved_entity.clone() else {
            return Ok(GraphExplainResponse {
                resolved_entity: None,
                narrative: None,
                chains: Vec::new(),
                evidence: Vec::new(),
                open_questions: vec![
                    "Resolve the ambiguous entity before generating an incident explanation."
                        .to_string(),
                ],
                missing_evidence: vec!["unique graph entity".to_string()],
                next_queries: Vec::new(),
                candidates,
                metadata: graph_metadata(
                    &status,
                    limits.as_graph_limits(),
                    true,
                    Some("ambiguous_entity".to_string()),
                ),
            });
        };

        let mut queue = VecDeque::new();
        queue.push_back(ExplainPath::root(root.id));
        let mut relationship_map: HashMap<i64, GraphRelationship> = HashMap::new();
        let mut entity_map: HashMap<i64, GraphEntity> = HashMap::from([(root.id, root.clone())]);
        let mut evidence_map: HashMap<i64, GraphEvidence> = HashMap::new();
        let mut completed_paths = Vec::new();
        let mut truncated = false;

        while let Some(path) = queue.pop_front() {
            if completed_paths.len() >= limits.max_chains as usize {
                truncated = true;
                break;
            }
            if path.depth >= limits.depth {
                if !path.relationship_ids.is_empty() {
                    completed_paths.push(path);
                }
                continue;
            }

            let entity_id = path.current_entity_id;
            let rows = self
                .run_db("graph.explain_around", move |pool| {
                    db::graph::graph_around_entity(
                        pool,
                        entity_id,
                        limits.beam_width,
                        limits.evidence_sample_limit,
                    )
                })
                .await?;
            truncated |= rows.truncated;
            let converted = graph_rows_to_models(rows, limits.payload_budget);
            for entity in converted.entities {
                entity_map.entry(entity.id).or_insert(entity);
            }
            for evidence in converted.evidence {
                evidence_map.entry(evidence.id).or_insert(evidence);
            }
            for relationship in converted.relationships {
                let next_id = if relationship.src_entity_id == entity_id {
                    relationship.dst_entity_id
                } else {
                    relationship.src_entity_id
                };
                relationship_map
                    .entry(relationship.id)
                    .or_insert_with(|| relationship.clone());
                if path.seen_entity_ids.contains(&next_id)
                    || path.relationship_ids.contains(&relationship.id)
                {
                    continue;
                }
                let mut next = path.clone();
                next.current_entity_id = next_id;
                next.depth += 1;
                next.seen_entity_ids.insert(next_id);
                next.relationship_ids.push(relationship.id);
                next.score += relationship_score(&relationship);
                queue.push_back(next);
            }
            if !path.relationship_ids.is_empty() {
                completed_paths.push(path);
            }
        }

        completed_paths.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        completed_paths.truncate(limits.max_chains as usize);

        let chains = completed_paths
            .into_iter()
            .enumerate()
            .map(|(idx, path)| {
                narrative_chain_from_path(idx + 1, &path, &entity_map, &relationship_map)
            })
            .collect::<Vec<_>>();
        let evidence = evidence_for_chains(&chains, &evidence_map);
        let narrative = build_graph_narrative(&root, &chains);
        let mut open_questions = graph_explain_open_questions(&chains);
        let mut missing_evidence = graph_explain_missing_evidence(&chains);
        if narrative.is_none() {
            missing_evidence.push("relationship evidence for a defensible explanation".to_string());
            open_questions.push("Which related entity should be inspected next?".to_string());
        }
        let next_queries = graph_explain_next_queries(&root, &entity_map);
        let payload_truncated = estimated_graph_explain_payload_bytes(&chains, &evidence)
            > limits.payload_budget as usize;
        Ok(GraphExplainResponse {
            resolved_entity: Some(root),
            narrative,
            chains,
            evidence,
            open_questions,
            missing_evidence,
            next_queries,
            candidates: Vec::new(),
            metadata: graph_metadata(
                &status,
                limits.as_graph_limits(),
                truncated || payload_truncated,
                if payload_truncated {
                    Some("payload_budget".to_string())
                } else if truncated {
                    Some("chain_limit".to_string())
                } else {
                    None
                },
            ),
        })
    }

    pub async fn graph_evidence_lookup(
        &self,
        req: GraphEvidenceLookupRequest,
    ) -> ServiceResult<GraphEvidenceLookupResponse> {
        if req.evidence_id <= 0 {
            return Err(ServiceError::InvalidInput(
                "evidence_id must be a positive integer".into(),
            ));
        }
        let limits = GraphLimits::for_evidence_lookup(req.payload_budget);
        let status = self
            .run_db("graph.status", db::graph::graph_projection_status)
            .await?;
        let evidence_id = req.evidence_id;
        let rows = self
            .run_db("graph.evidence_lookup", move |pool| {
                db::graph::graph_evidence_by_id(pool, evidence_id)
            })
            .await?
            .ok_or_else(|| ServiceError::NotFound("graph evidence not found".into()))?;
        let src_entity = GraphEntity::from(rows.src_entity);
        let dst_entity = GraphEntity::from(rows.dst_entity);
        let src_summary = GraphEntitySummary::from(&src_entity);
        let dst_summary = GraphEntitySummary::from(&dst_entity);
        let evidence = graph_evidence_safe(rows.evidence, limits.payload_budget);
        let relationship = graph_relationship_to_model(
            rows.relationship,
            Some(src_summary.clone()),
            Some(dst_summary.clone()),
            vec![evidence.id],
        );
        let source_log_summary = rows
            .source_log_summary
            .map(|row| graph_source_log_summary_safe(row, limits.payload_budget));
        let missing_source_reason = if evidence.source_log_id.is_none() {
            Some("evidence_source_is_not_a_log".to_string())
        } else if source_log_summary.is_none() {
            Some("source_log_missing_or_retained_out".to_string())
        } else {
            None
        };
        let payload_truncated = estimated_graph_evidence_lookup_payload_bytes(
            &relationship,
            &evidence,
            &src_summary,
            &dst_summary,
            source_log_summary.as_ref(),
        ) > limits.payload_budget as usize;
        Ok(GraphEvidenceLookupResponse {
            evidence,
            relationship,
            src_entity: src_summary,
            dst_entity: dst_summary,
            source_log_summary,
            missing_source_reason,
            metadata: graph_metadata(
                &status,
                limits,
                payload_truncated,
                payload_truncated.then(|| "payload_budget".to_string()),
            ),
        })
    }

    pub async fn graph_projection_status(&self) -> ServiceResult<GraphProjectionStatusResponse> {
        let status = self
            .run_db("graph.status", db::graph::graph_projection_status)
            .await?;
        Ok(graph_projection_status_response(status))
    }

    pub async fn graph_rebuild(&self) -> ServiceResult<GraphRebuildResponse> {
        let mut outcome = self
            .run_db("graph.rebuild", db::graph::refresh_graph_projection)
            .await?;
        if matches!(&outcome, db::graph::GraphRebuildOutcome::Rebuilt(_)) {
            let config = crate::inventory::InventoryConfig::from_env();
            let cache_path =
                crate::inventory::storage::InventoryPaths::new(config.root.clone()).normalized_json;
            match crate::inventory::read_inventory_cache(&config) {
                Ok(inventory) => {
                    let project_result = self
                        .run_db("graph.inventory_project", move |pool| {
                            db::graph_inventory::project_inventory(pool, &inventory)
                        })
                        .await;
                    if let Err(error) = project_result {
                        tracing::warn!(%error, "graph rebuild inventory projection failed");
                        let error_message = error.to_string();
                        let mark_result = self
                            .run_db("graph.inventory_project_failed", move |pool| {
                                db::graph_inventory::mark_inventory_projection_failed(
                                    pool,
                                    &error_message,
                                )
                            })
                            .await;
                        if let Err(mark_error) = mark_result {
                            tracing::warn!(
                                %mark_error,
                                "graph rebuild failed to mark inventory projection degraded"
                            );
                        }
                        return Err(error);
                    }
                    if let db::graph::GraphRebuildOutcome::Rebuilt(stats) = &mut outcome {
                        let final_status = self
                            .run_db("graph.status", db::graph::graph_projection_status)
                            .await?;
                        stats.entity_count = final_status.entity_count;
                        stats.relationship_count = final_status.relationship_count;
                        stats.evidence_count = final_status.evidence_count;
                    }
                }
                Err(error) if !cache_path.exists() => {
                    tracing::debug!(
                        %error,
                        path = %cache_path.display(),
                        "graph rebuild skipped inventory projection; cache unavailable"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        path = %cache_path.display(),
                        "graph rebuild failed to read inventory cache"
                    );
                    return Err(error.into());
                }
            }
        }
        let status = self.graph_projection_status().await?;
        let (outcome, stats) = match outcome {
            db::graph::GraphRebuildOutcome::Rebuilt(stats) => (
                "rebuilt".to_string(),
                Some(graph_rebuild_stats_response(stats)),
            ),
            db::graph::GraphRebuildOutcome::AlreadyRunning => ("already_running".to_string(), None),
        };
        Ok(GraphRebuildResponse {
            outcome,
            stats,
            status,
        })
    }
}
