use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct GraphLimits {
    pub(super) limit: u32,
    pub(super) depth: u32,
    pub(super) evidence_sample_limit: u32,
    pub(super) payload_budget: u32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct GraphExplainLimits {
    pub(super) depth: u32,
    pub(super) beam_width: u32,
    pub(super) max_chains: u32,
    pub(super) evidence_sample_limit: u32,
    pub(super) payload_budget: u32,
}

impl GraphExplainLimits {
    pub(super) fn from_request(req: &GraphExplainRequest) -> Self {
        Self {
            depth: req.depth.unwrap_or(2).clamp(1, 3),
            beam_width: req.beam_width.unwrap_or(20).clamp(1, 100),
            max_chains: req.max_chains.unwrap_or(200).clamp(1, 200),
            evidence_sample_limit: req.evidence_sample_limit.unwrap_or(2).clamp(0, 5),
            payload_budget: req.payload_budget.unwrap_or(32_768).clamp(4_096, 65_536),
        }
    }

    pub(super) fn as_graph_limits(self) -> GraphLimits {
        GraphLimits {
            limit: self.max_chains,
            depth: self.depth,
            evidence_sample_limit: self.evidence_sample_limit,
            payload_budget: self.payload_budget,
        }
    }
}

impl GraphLimits {
    pub(super) fn for_evidence_lookup(payload_budget: Option<u32>) -> Self {
        Self {
            limit: 1,
            depth: 0,
            evidence_sample_limit: 1,
            payload_budget: payload_budget.unwrap_or(32_768).clamp(4_096, 65_536),
        }
    }

    pub(super) fn from_entity_request(req: &GraphEntityLookupRequest) -> Self {
        Self {
            limit: req.limit.unwrap_or(20).clamp(1, 100),
            depth: 0,
            evidence_sample_limit: req.evidence_sample_limit.unwrap_or(3).clamp(0, 5),
            payload_budget: req.payload_budget.unwrap_or(32_768).clamp(4_096, 65_536),
        }
    }

    pub(super) fn from_around_request(req: &GraphAroundRequest) -> ServiceResult<Self> {
        let depth = req.depth.unwrap_or(1);
        if depth > 1 {
            return Err(ServiceError::InvalidInput(
                "graph around supports depth=1 only in v1".into(),
            ));
        }
        Ok(Self {
            limit: req.limit.unwrap_or(100).clamp(1, 500),
            depth,
            evidence_sample_limit: req.evidence_sample_limit.unwrap_or(3).clamp(0, 5),
            payload_budget: req.payload_budget.unwrap_or(32_768).clamp(4_096, 65_536),
        })
    }
}

#[derive(Debug, Clone)]
pub(super) struct ExplainPath {
    pub(super) current_entity_id: i64,
    pub(super) depth: u32,
    pub(super) seen_entity_ids: HashSet<i64>,
    pub(super) relationship_ids: Vec<i64>,
    pub(super) score: f64,
}

impl ExplainPath {
    pub(super) fn root(entity_id: i64) -> Self {
        Self {
            current_entity_id: entity_id,
            depth: 0,
            seen_entity_ids: HashSet::from([entity_id]),
            relationship_ids: Vec::new(),
            score: 0.0,
        }
    }
}

pub(super) struct GraphRowsModels {
    pub(super) relationships: Vec<GraphRelationship>,
    pub(super) entities: Vec<GraphEntity>,
    pub(super) evidence: Vec<GraphEvidence>,
}
