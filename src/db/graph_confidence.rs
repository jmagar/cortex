//! Pure confidence math for the investigation graph.
//!
//! All functions here are pure (no DB, no clock): they take stored values and
//! return derived confidences. Temporal decay and evidence combination are
//! applied at *query time* on top of the stored peak confidence — nothing here
//! mutates persisted data, so there is no schema impact.
//!
//! Three ideas:
//! - **Noisy-OR** combines confidences from *independent* sources:
//!   `1 - Π(1 - cᵢ)`. Monotonic, bounded `[0,1]`, rewards corroboration.
//! - **BEWA diminishing returns** collapses *same-source* repetition: 1000
//!   syslog lines are one fact seen 1000 times, not 1000 independent facts.
//!   Each doubling of `evidence_count` adds one effective observation.
//! - **CountTRuCoLa-style temporal decay** ages edges toward a floor with a
//!   per-relationship half-life, so stale `runs_on` edges fade while structural
//!   `worked_on` edges persist.

use super::graph::{
    REASON_AGENT_COMMAND_CWD_INFER, REASON_AGENT_COMMAND_GIT_COMMIT, REASON_AGENT_COMMAND_SESSION,
    REASON_AI_SESSION_PROJECT, REASON_COMPOSE_CONFIG, REASON_DOCKER_CONTAINER_ID,
    REASON_DOCKER_NETWORK, REASON_DOCKER_SERVICE_LABEL, REASON_ERROR_SIGNATURE_MATCH,
    REASON_HEARTBEAT_HOST_STATE, REASON_LOG_APP_NAME, REASON_REVERSE_PROXY_CONFIG,
    REASON_SHELL_HISTORY_GIT_COMMIT, REASON_SYSLOG_CLAIMED_HOSTNAME,
};

/// ln(2), the half-life constant for an exponential `exp(-λt)` decay.
const LN2: f64 = std::f64::consts::LN_2;

/// Effective-confidence ceiling for `correlated`-trust edges. `correlated` marks
/// a derivation *method* (temporal co-occurrence), not a verified fact, so its
/// confidence is capped well below structural edges.
pub(crate) const TRUST_CORRELATED_CEILING: f64 = 0.5;

/// Cap a confidence by trust level: `refuted` edges contribute nothing,
/// `correlated` edges are capped at `TRUST_CORRELATED_CEILING`, everything else
/// passes through. Use after computing effective confidence.
pub(crate) fn apply_trust_ceiling(confidence: f64, trust_level: &str) -> f64 {
    use super::graph::{TRUST_CORRELATED, TRUST_REFUTED};
    match trust_level {
        TRUST_REFUTED => 0.0,
        TRUST_CORRELATED => confidence.min(TRUST_CORRELATED_CEILING),
        _ => confidence,
    }
}

/// Combine independent confidences via noisy-OR: `1 - Π(1 - cᵢ)`.
///
/// A single source is returned unchanged; corroborating sources push the result
/// up toward (never past) 1.0. Inputs are clamped to `[0, 1]`; an empty slice
/// yields 0.0.
pub(crate) fn noisy_or_combine(confidences: &[f64]) -> f64 {
    let product = confidences
        .iter()
        .map(|c| 1.0 - c.clamp(0.0, 1.0))
        .product::<f64>();
    (1.0 - product).clamp(0.0, 1.0)
}

/// BEWA diminishing returns: the effective independent-observation count implied
/// by a raw same-source `evidence_count`. `log2(1 + n)` — each doubling of
/// same-source evidence adds one effective unit (1→1, 1000→~10).
pub(crate) fn bewa_effective_count(evidence_count: i64) -> f64 {
    if evidence_count <= 0 {
        return 0.0;
    }
    (1.0 + evidence_count as f64).ln() / LN2
}

/// Confidence accumulated from `evidence_count` same-source observations, each
/// of `per_observation` confidence, with BEWA diminishing returns folded into a
/// noisy-OR: `1 - (1 - p)^effective_count`.
pub(crate) fn confidence_from_repeated(per_observation: f64, evidence_count: i64) -> f64 {
    let p = per_observation.clamp(0.0, 1.0);
    let n = bewa_effective_count(evidence_count);
    (1.0 - (1.0 - p).powf(n)).clamp(0.0, 1.0)
}

/// Per-hour decay rate `λ = ln2 / half_life_hours` for a reason code. `0` means
/// the edge never decays (structural facts like session→project).
pub(crate) fn decay_lambda_per_hour(reason_code: &str) -> f64 {
    let half_life_hours = match reason_code {
        // Volatile runtime topology — a container's host can change on restart.
        REASON_DOCKER_CONTAINER_ID | REASON_DOCKER_SERVICE_LABEL => 0.25,
        // Recent observations that age over a day.
        REASON_LOG_APP_NAME | REASON_SYSLOG_CLAIMED_HOSTNAME => 24.0,
        // Point-in-time signals decay fast.
        REASON_ERROR_SIGNATURE_MATCH | REASON_HEARTBEAT_HOST_STATE => 1.0,
        // Config-derived structure is stable for weeks.
        REASON_COMPOSE_CONFIG | REASON_REVERSE_PROXY_CONFIG | REASON_DOCKER_NETWORK => 720.0,
        // Structural / FK-backed facts never decay.
        REASON_AI_SESSION_PROJECT
        | REASON_AGENT_COMMAND_SESSION
        | REASON_AGENT_COMMAND_CWD_INFER
        | REASON_AGENT_COMMAND_GIT_COMMIT
        | REASON_SHELL_HISTORY_GIT_COMMIT => return 0.0,
        // Default: slow weekly decay for anything unlisted.
        _ => 168.0,
    };
    LN2 / half_life_hours
}

/// Asymptotic confidence floor `φ` for a reason code — the minimum the edge
/// decays toward as `Δt → ∞`. Point-in-time signals fall to 0; most edges keep
/// a small residual.
pub(crate) fn asymptotic_floor(reason_code: &str) -> f64 {
    match reason_code {
        REASON_ERROR_SIGNATURE_MATCH | REASON_HEARTBEAT_HOST_STATE => 0.0,
        _ => 0.1,
    }
}

/// Recency factor in `[φ, 1]`: `φ + (1 - φ)·exp(-λ·Δt)`. `λ = 0` (never-decay
/// edges) returns exactly 1.0; `Δt ≤ 0` returns 1.0.
pub(crate) fn compute_recency(lambda_per_hour: f64, delta_hours: f64, phi: f64) -> f64 {
    if lambda_per_hour <= 0.0 || delta_hours <= 0.0 {
        return 1.0;
    }
    let phi = phi.clamp(0.0, 1.0);
    phi + (1.0 - phi) * (-lambda_per_hour * delta_hours).exp()
}

/// Query-time effective confidence: `stored × recency(reason_code, Δt)`.
/// `delta_hours` is `(now − last_seen_at)` in hours, computed by the caller.
pub(crate) fn compute_effective_confidence(
    stored: f64,
    reason_code: &str,
    delta_hours: f64,
) -> f64 {
    let lambda = decay_lambda_per_hour(reason_code);
    let phi = asymptotic_floor(reason_code);
    stored.clamp(0.0, 1.0) * compute_recency(lambda, delta_hours, phi)
}

#[cfg(test)]
#[path = "graph_confidence_tests.rs"]
mod tests;
