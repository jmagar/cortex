use super::*;

impl CortexService {
    pub(super) async fn homelab_map_graph_answer(
        &self,
        req: &HomelabMapRequest,
    ) -> ServiceResult<Option<HomelabMapGraphAnswer>> {
        let Some(mode) = req
            .mode
            .as_deref()
            .map(str::trim)
            .filter(|mode| !mode.is_empty())
        else {
            return Ok(None);
        };
        if mode == "snapshot" {
            return Ok(None);
        }

        let (entity_type, key) = match mode {
            "host_services" => (
                "host".to_string(),
                required_map_target(req.host.as_deref(), "host", mode)?,
            ),
            "domain_routes" => (
                "domain".to_string(),
                required_map_target(req.domain.as_deref(), "domain", mode)?,
            ),
            "service_dependencies" => (
                "service".to_string(),
                service_dependency_key(req.host.as_deref(), req.service.as_deref())?,
            ),
            _ => {
                return Err(ServiceError::InvalidInput(format!(
                    "unsupported map mode `{mode}`; expected snapshot, host_services, domain_routes, or service_dependencies"
                )));
            }
        };

        let graph = self
            .graph_around(GraphAroundRequest {
                mode: Some("around".to_string()),
                entity_type: Some(entity_type.clone()),
                key: Some(key.clone()),
                depth: Some(1),
                limit: req.answer_limit,
                evidence_sample_limit: req.evidence_sample_limit,
                payload_budget: req.payload_budget,
                ..Default::default()
            })
            .await?;

        Ok(Some(map_graph_answer(
            mode,
            HomelabMapGraphTarget { entity_type, key },
            graph,
        )))
    }
}

fn required_map_target(value: Option<&str>, field: &str, mode: &str) -> ServiceResult<String> {
    let value = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ServiceError::InvalidInput(format!("map mode `{mode}` requires `{field}`"))
        })?;
    Ok(value.to_string())
}

fn service_dependency_key(host: Option<&str>, service: Option<&str>) -> ServiceResult<String> {
    let service = required_map_target(service, "service", "service_dependencies")?;
    if service.contains(':') {
        return Ok(service);
    }
    let host = required_map_target(host, "host", "service_dependencies")?;
    Ok(format!("{host}:{service}"))
}

fn map_graph_answer(
    mode: &str,
    target: HomelabMapGraphTarget,
    graph: GraphAroundResponse,
) -> HomelabMapGraphAnswer {
    let answer_status = if graph.resolved_entity.is_some() {
        "ok"
    } else if graph.candidates.is_empty() {
        "not_found"
    } else {
        "ambiguous"
    }
    .to_string();
    let rows = map_answer_rows(&graph);
    let next_queries = map_next_queries(mode, &target, &rows);
    let proof_queries = map_proof_queries(&graph);
    let metadata = graph.metadata.clone();
    HomelabMapGraphAnswer {
        mode: mode.to_string(),
        answer_status,
        target,
        rows,
        candidates: graph.candidates,
        evidence: graph.evidence,
        metadata: metadata.clone(),
        truncation: HomelabMapAnswerTruncation {
            truncated: metadata.truncated,
            reason: metadata.truncated_reason.clone(),
            limit: metadata.limit,
            evidence_sample_limit: metadata.evidence_sample_limit,
            payload_budget: metadata.payload_budget,
        },
        degraded_reason: metadata.is_degraded.then(|| {
            metadata
                .last_error
                .clone()
                .unwrap_or_else(|| "graph_degraded".to_string())
        }),
        next_queries,
        proof_queries,
    }
}

fn map_answer_rows(graph: &GraphAroundResponse) -> Vec<HomelabMapAnswerRow> {
    let Some(resolved) = &graph.resolved_entity else {
        return Vec::new();
    };
    graph
        .relationships
        .iter()
        .filter_map(|relationship| map_answer_row(resolved.id, relationship))
        .collect()
}

fn map_answer_row(
    resolved_entity_id: i64,
    relationship: &GraphRelationship,
) -> Option<HomelabMapAnswerRow> {
    let (entity, direction) = if relationship.src_entity_id == resolved_entity_id {
        (relationship.dst_entity.as_ref()?, "outgoing")
    } else if relationship.dst_entity_id == resolved_entity_id {
        (relationship.src_entity.as_ref()?, "incoming")
    } else {
        return None;
    };
    Some(HomelabMapAnswerRow {
        entity_type: entity.entity_type.clone(),
        key: entity.canonical_key.clone(),
        label: entity.display_label.clone(),
        relationship_type: relationship.relationship_type.clone(),
        direction: direction.to_string(),
        trust_level: relationship.trust_level.clone(),
        confidence: relationship.confidence,
        evidence_ids: relationship.evidence_ids.clone(),
    })
}

fn map_next_queries(
    mode: &str,
    target: &HomelabMapGraphTarget,
    rows: &[HomelabMapAnswerRow],
) -> Vec<HomelabMapNextQuery> {
    std::iter::once(target_next_query(mode, target))
        .flatten()
        .chain(
            rows.iter()
                .filter_map(|row| match row.entity_type.as_str() {
                    "host" => Some(HomelabMapNextQuery {
                        action: "map".to_string(),
                        mode: "host_services".to_string(),
                        host: Some(row.key.clone()),
                        domain: None,
                        service: None,
                        reason: format!("inspect services on {}", row.label),
                    }),
                    "domain" => Some(HomelabMapNextQuery {
                        action: "map".to_string(),
                        mode: "domain_routes".to_string(),
                        host: None,
                        domain: Some(row.key.clone()),
                        service: None,
                        reason: format!("inspect route for {}", row.label),
                    }),
                    "service" => Some(HomelabMapNextQuery {
                        action: "map".to_string(),
                        mode: "service_dependencies".to_string(),
                        host: None,
                        domain: None,
                        service: Some(row.key.clone()),
                        reason: format!("inspect dependencies for {}", row.label),
                    }),
                    _ => None,
                }),
        )
        .take(10)
        .collect()
}

fn target_next_query(mode: &str, target: &HomelabMapGraphTarget) -> Option<HomelabMapNextQuery> {
    match (mode, target.entity_type.as_str()) {
        ("host_services", "host") => Some(HomelabMapNextQuery {
            action: "map".to_string(),
            mode: mode.to_string(),
            host: Some(target.key.clone()),
            domain: None,
            service: None,
            reason: "refresh this host service view".to_string(),
        }),
        ("domain_routes", "domain") => Some(HomelabMapNextQuery {
            action: "map".to_string(),
            mode: mode.to_string(),
            host: None,
            domain: Some(target.key.clone()),
            service: None,
            reason: "refresh this domain route view".to_string(),
        }),
        ("service_dependencies", "service") => Some(HomelabMapNextQuery {
            action: "map".to_string(),
            mode: mode.to_string(),
            host: None,
            domain: None,
            service: Some(target.key.clone()),
            reason: "refresh this service dependency view".to_string(),
        }),
        _ => None,
    }
}

fn map_proof_queries(graph: &GraphAroundResponse) -> Vec<HomelabMapProofQuery> {
    let mut proof_queries = Vec::new();
    if let Some(entity) = &graph.resolved_entity {
        proof_queries.push(HomelabMapProofQuery {
            action: "graph".to_string(),
            mode: "around".to_string(),
            entity_id: Some(entity.id),
            evidence_id: None,
            label: format!("graph around {}", entity.display_label),
        });
    }
    proof_queries.extend(
        graph
            .evidence
            .iter()
            .take(5)
            .map(|evidence| HomelabMapProofQuery {
                action: "graph".to_string(),
                mode: "evidence".to_string(),
                entity_id: None,
                evidence_id: Some(evidence.id),
                label: format!("evidence {}", evidence.id),
            }),
    );
    proof_queries
}
