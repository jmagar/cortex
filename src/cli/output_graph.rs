use anyhow::Result;
use cortex::app::{
    GraphAroundResponse, GraphEntity, GraphEntityLookupResponse, GraphEntitySummary, GraphEvidence,
    GraphEvidenceLookupResponse, GraphExplainResponse, GraphProjectionStatusResponse,
    GraphRebuildResponse, GraphRelationship,
};

use super::color::{cyan, muted, primary, warn};
use super::output_common::{print_json, truncate};

pub(crate) fn print_graph_status_response(
    response: &GraphProjectionStatusResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{}={} degraded={} completed={} watermark={}",
        muted("projection"),
        primary(&safe_display(&response.projection_status)),
        response.is_degraded,
        muted(response.last_completed_at.as_deref().unwrap_or("-")),
        muted(&safe_display(&response.source_watermark)),
    );
    println!(
        "source_rows={} entities={} relationships={} evidence={} chunks={} runtime_ms={}",
        cyan(&response.source_row_count.to_string()),
        cyan(&response.entity_count.to_string()),
        cyan(&response.relationship_count.to_string()),
        cyan(&response.evidence_count.to_string()),
        cyan(&response.last_chunk_count.to_string()),
        cyan(&response.last_runtime_ms.to_string()),
    );
    if let Some(err) = &response.last_error {
        println!("{}: {}", warn("projection_error"), safe_display(err));
    }
    Ok(())
}

pub(crate) fn print_graph_rebuild_response(
    response: &GraphRebuildResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("{}={}", muted("rebuild"), primary(&response.outcome));
    if let Some(stats) = &response.stats {
        println!(
            "source_rows={} entities={} relationships={} evidence={} chunks={} runtime_ms={}",
            cyan(&stats.source_row_count.to_string()),
            cyan(&stats.entity_count.to_string()),
            cyan(&stats.relationship_count.to_string()),
            cyan(&stats.evidence_count.to_string()),
            cyan(&stats.chunk_count.to_string()),
            cyan(&stats.runtime_ms.to_string()),
        );
    }
    print_graph_status_response(&response.status, false)
}

pub(crate) fn print_graph_entity_lookup_response(
    response: &GraphEntityLookupResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    print_graph_metadata(&response.metadata);
    if let Some(entity) = &response.resolved_entity {
        println!("{}", entity_line("resolved", entity));
    }
    if !response.candidates.is_empty() {
        println!("{}", muted("candidates:"));
        for candidate in &response.candidates {
            println!(
                "  {} match={} alias={}:{}",
                entity_line("-", &candidate.entity),
                primary(&safe_display(&candidate.match_reason)),
                muted(&safe_display(
                    candidate.alias_type.as_deref().unwrap_or("-")
                )),
                muted(&safe_display(candidate.alias_key.as_deref().unwrap_or("-")))
            );
        }
    }
    Ok(())
}

pub(crate) fn print_graph_around_response(
    response: &GraphAroundResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    print_graph_metadata(&response.metadata);
    if let Some(entity) = &response.resolved_entity {
        println!("{}", entity_line("resolved", entity));
    }
    if !response.candidates.is_empty() {
        println!("{}", muted("ambiguous candidates:"));
        for candidate in &response.candidates {
            println!(
                "  {} match={}",
                entity_line("-", &candidate.entity),
                primary(&safe_display(&candidate.match_reason))
            );
        }
        return Ok(());
    }

    println!(
        "{} relationship(s), {} related entity record(s), {} evidence sample(s)",
        cyan(&response.relationships.len().to_string()),
        cyan(&response.entities.len().to_string()),
        cyan(&response.evidence.len().to_string())
    );
    for relationship in &response.relationships {
        print_relationship(relationship, &response.entities, &response.evidence);
    }
    if !response.next_queries.is_empty() {
        println!("{}", muted("follow-ups:"));
        for query in &response.next_queries {
            println!(
                "  cortex graph around --entity-id {}  # {}",
                query.entity_id,
                safe_display(&query.label)
            );
        }
    }
    Ok(())
}

pub(crate) fn print_graph_explain_response(
    response: &GraphExplainResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    print_graph_metadata(&response.metadata);
    if let Some(entity) = &response.resolved_entity {
        println!("{}", entity_line("resolved", entity));
    }
    if !response.candidates.is_empty() {
        println!("{}", muted("ambiguous candidates:"));
        for candidate in &response.candidates {
            println!(
                "  {} match={}",
                entity_line("-", &candidate.entity),
                primary(&safe_display(&candidate.match_reason))
            );
        }
        return Ok(());
    }
    if let Some(narrative) = &response.narrative {
        println!(
            "{} confidence={}",
            primary(&safe_display(&narrative.title)),
            cyan(&safe_display(&narrative.confidence))
        );
        println!("{}", safe_display(&narrative.summary));
        println!(
            "{} relationships={:?} evidence={:?}",
            muted("cites"),
            narrative.relationship_ids,
            narrative.evidence_ids
        );
    } else {
        println!("{}", warn("no evidence-backed narrative generated"));
    }
    for chain in &response.chains {
        println!(
            "\n{} confidence={} score={:.2}",
            primary(&safe_display(&chain.chain_id)),
            cyan(&safe_display(&chain.confidence)),
            chain.score
        );
        println!("  {}", safe_display(&chain.summary));
        println!(
            "  {} relationships={:?} evidence={:?}",
            muted("cites"),
            chain.relationship_ids,
            chain.evidence_ids
        );
        for relationship in &chain.relationships {
            let src = relationship
                .src_entity
                .as_ref()
                .map(entity_summary_label)
                .unwrap_or_else(|| format!("#{}", relationship.src_entity_id));
            let dst = relationship
                .dst_entity
                .as_ref()
                .map(entity_summary_label)
                .unwrap_or_else(|| format!("#{}", relationship.dst_entity_id));
            println!(
                "  {} {} -> {} trust={} reason={} evidence={}",
                safe_display(&relationship.relationship_type),
                src,
                dst,
                safe_display(&relationship.trust_level),
                safe_display(&relationship.reason_code),
                relationship.evidence_count
            );
        }
    }
    if !response.missing_evidence.is_empty() {
        println!("{}", muted("missing evidence:"));
        for item in &response.missing_evidence {
            println!("  - {}", safe_display(item));
        }
    }
    if !response.open_questions.is_empty() {
        println!("{}", muted("open questions:"));
        for item in &response.open_questions {
            println!("  - {}", safe_display(item));
        }
    }
    if !response.next_queries.is_empty() {
        println!("{}", muted("follow-ups:"));
        for query in &response.next_queries {
            println!(
                "  cortex graph around --entity-id {}  # {}",
                query.entity_id,
                safe_display(&query.label)
            );
        }
    }
    Ok(())
}

pub(crate) fn print_graph_evidence_lookup_response(
    response: &GraphEvidenceLookupResponse,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    print_graph_metadata(&response.metadata);
    let rel = &response.relationship;
    let src = rel
        .src_entity
        .as_ref()
        .map(entity_summary_label)
        .unwrap_or_else(|| entity_summary_label(&response.src_entity));
    let dst = rel
        .dst_entity
        .as_ref()
        .map(entity_summary_label)
        .unwrap_or_else(|| entity_summary_label(&response.dst_entity));
    println!(
        "{} #{} relationship #{}",
        primary("evidence"),
        response.evidence.id,
        rel.id
    );
    println!(
        "{} {} {}",
        src,
        primary(&safe_display(&rel.relationship_type)),
        dst
    );
    println!(
        "reason={} trust={} confidence={:.2} evidence_count={}",
        safe_display(&response.evidence.reason_code),
        safe_display(&response.evidence.trust_level),
        rel.confidence,
        response.evidence.evidence_count
    );
    if let Some(reason) = &response.evidence.reason_text {
        println!("reason_text={}", safe_display(&truncate(reason, 160)));
    }
    println!(
        "source kind={} id={} log_id={} heartbeat_id={} signature={}",
        safe_display(&response.evidence.source_kind),
        safe_display(&truncate(&response.evidence.source_id, 80)),
        response
            .evidence
            .source_log_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".into()),
        response
            .evidence
            .source_heartbeat_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".into()),
        safe_display(
            response
                .evidence
                .source_signature_hash
                .as_deref()
                .unwrap_or("-")
        )
    );
    if let Some(path) = &response.evidence.metadata_path {
        println!("metadata_path={}", safe_display(&truncate(path, 120)));
    }
    if let Some(excerpt) = &response.evidence.safe_excerpt {
        println!("excerpt={}", safe_display(&truncate(excerpt, 160)));
    }
    if let Some(summary) = &response.source_log_summary {
        println!(
            "source_log #{} {} host={} severity={} app={} source_ip={}",
            summary.id,
            muted(&safe_display(&summary.timestamp)),
            safe_display(&summary.hostname),
            safe_display(&summary.severity),
            safe_display(summary.app_name.as_deref().unwrap_or("-")),
            safe_display(&summary.source_ip)
        );
        println!("  {}", safe_display(&truncate(&summary.message, 200)));
        if summary.message_truncated {
            println!("  {}", muted("message_truncated=true"));
        }
    } else if let Some(reason) = &response.missing_source_reason {
        println!("source_log_summary=null reason={}", safe_display(reason));
    }
    println!("{}", muted("follow-ups:"));
    println!("  cortex graph around --entity-id {}", rel.src_entity_id);
    println!("  cortex graph around --entity-id {}", rel.dst_entity_id);
    println!("  cortex graph explain --entity-id {}", rel.src_entity_id);
    Ok(())
}

pub(crate) fn safe_display(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| {
            if ch.is_control() {
                ch.escape_default().collect::<Vec<_>>()
            } else {
                vec![ch]
            }
        })
        .collect()
}

fn print_graph_metadata(metadata: &cortex::app::GraphResponseMetadata) {
    let degraded = if metadata.is_degraded {
        format!(" {}", warn("degraded"))
    } else {
        String::new()
    };
    println!(
        "{}={}{} completed={} watermark={}",
        muted("projection"),
        primary(&safe_display(&metadata.projection_status)),
        degraded,
        muted(metadata.last_completed_at.as_deref().unwrap_or("-")),
        muted(&safe_display(&metadata.source_watermark)),
    );
    if metadata.truncated {
        println!(
            "{}: {}",
            warn("truncated"),
            safe_display(metadata.truncated_reason.as_deref().unwrap_or("limit"))
        );
    }
    if let Some(err) = &metadata.last_error {
        println!("{}: {}", warn("projection_error"), safe_display(err));
    }
}

fn entity_line(prefix: &str, entity: &GraphEntity) -> String {
    format!(
        "{} {}:{} id={} label={} trust={} source={}/{}",
        muted(prefix),
        cyan(&safe_display(&entity.entity_type)),
        primary(&safe_display(&entity.canonical_key)),
        entity.id,
        safe_display(&truncate(&entity.display_label, 48)),
        safe_display(&entity.trust_level),
        safe_display(&entity.source_kind),
        safe_display(&truncate(&entity.source_id, 48))
    )
}

fn print_relationship(
    relationship: &GraphRelationship,
    entities: &[GraphEntity],
    evidence: &[GraphEvidence],
) {
    let src = relationship
        .src_entity
        .as_ref()
        .map(entity_summary_label)
        .or_else(|| {
            entities
                .iter()
                .find(|entity| entity.id == relationship.src_entity_id)
                .map(entity_label)
        })
        .unwrap_or_else(|| format!("#{}", relationship.src_entity_id));
    let dst = relationship
        .dst_entity
        .as_ref()
        .map(entity_summary_label)
        .or_else(|| {
            entities
                .iter()
                .find(|entity| entity.id == relationship.dst_entity_id)
                .map(entity_label)
        })
        .unwrap_or_else(|| format!("#{}", relationship.dst_entity_id));
    println!(
        "\n{} {} -> {} confidence={:.2} trust={} reason={} evidence={}",
        primary(&safe_display(&relationship.relationship_type)),
        src,
        dst,
        relationship.confidence,
        safe_display(&relationship.trust_level),
        safe_display(&relationship.reason_code),
        relationship.evidence_count
    );
    for sample in evidence
        .iter()
        .filter(|item| item.relationship_id == relationship.id)
        .take(3)
    {
        let reason = sample.reason_text.as_deref().unwrap_or(&sample.reason_code);
        let excerpt = sample.safe_excerpt.as_deref().unwrap_or("-");
        println!(
            "  evidence #{} {} {} source={} excerpt={}",
            sample.id,
            muted(&safe_display(&sample.observed_at)),
            safe_display(reason),
            safe_display(&truncate(&sample.source_id, 48)),
            safe_display(&truncate(excerpt, 96)),
        );
    }
}

fn entity_summary_label(entity: &GraphEntitySummary) -> String {
    format!(
        "{}:{}",
        cyan(&safe_display(&entity.entity_type)),
        primary(&safe_display(&truncate(&entity.display_label, 40)))
    )
}

fn entity_label(entity: &GraphEntity) -> String {
    format!(
        "{}:{}",
        cyan(&safe_display(&entity.entity_type)),
        primary(&safe_display(&truncate(&entity.display_label, 40)))
    )
}

#[cfg(test)]
#[path = "output_graph_tests.rs"]
mod tests;
