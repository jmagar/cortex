//! Tests for the pure graph confidence math.

use super::*;
use crate::db::graph::{
    REASON_AI_SESSION_PROJECT, REASON_DOCKER_CONTAINER_ID, REASON_ERROR_SIGNATURE_MATCH,
};

#[test]
fn noisy_or_single_source_is_unchanged() {
    assert!((noisy_or_combine(&[0.5]) - 0.5).abs() < 1e-9);
    assert!((noisy_or_combine(&[0.9]) - 0.9).abs() < 1e-9);
    assert_eq!(noisy_or_combine(&[]), 0.0);
}

#[test]
fn noisy_or_corroboration_increases_but_stays_bounded() {
    let combined = noisy_or_combine(&[0.9, 0.9]);
    assert!(
        combined > 0.9,
        "corroboration must raise confidence: {combined}"
    );
    assert!(combined < 1.0, "noisy-OR is bounded below 1.0: {combined}");
    // 1 - (0.1 * 0.1) = 0.99
    assert!((combined - 0.99).abs() < 1e-9);
}

#[test]
fn bewa_diminishes_same_source_repetition() {
    // log2(1+1000) ≈ 9.97 — a thousand lines is ~10 effective observations.
    let n = bewa_effective_count(1000);
    assert!(n > 9.0 && n < 11.0, "effective count was {n}");
    assert!(
        n < 1000.0 * 0.1,
        "diminishing returns must be far below raw count"
    );
    assert_eq!(bewa_effective_count(0), 0.0);
    assert_eq!(bewa_effective_count(-5), 0.0);

    // Repeated weak evidence accumulates but never exceeds 1.0.
    let c = confidence_from_repeated(0.3, 1000);
    assert!(c > 0.3 && c < 1.0, "repeated confidence was {c}");
}

#[test]
fn recency_decays_monotonically_to_floor() {
    let lambda = decay_lambda_per_hour(REASON_DOCKER_CONTAINER_ID);
    let phi = asymptotic_floor(REASON_DOCKER_CONTAINER_ID);
    let now = compute_recency(lambda, 0.0, phi);
    let quarter = compute_recency(lambda, 0.25, phi); // one 15-min half-life
    let far = compute_recency(lambda, 100.0, phi);

    assert!((now - 1.0).abs() < 1e-9, "Δt=0 → full recency");
    // At one half-life the (1-φ) component halves: φ + (1-φ)*0.5.
    let expected = phi + (1.0 - phi) * 0.5;
    assert!(
        (quarter - expected).abs() < 1e-6,
        "half-life recency {quarter}"
    );
    assert!(quarter < now, "decays over time");
    assert!((far - phi).abs() < 1e-3, "approaches the floor {far}");
}

#[test]
fn never_decay_reason_keeps_full_confidence() {
    // ai_session_project (λ=0) does not decay even after a long gap.
    let lambda = decay_lambda_per_hour(REASON_AI_SESSION_PROJECT);
    assert_eq!(lambda, 0.0);
    let eff = compute_effective_confidence(0.9, REASON_AI_SESSION_PROJECT, 10_000.0);
    assert!(
        (eff - 0.9).abs() < 1e-9,
        "structural edge must not decay: {eff}"
    );
}

#[test]
fn structural_old_edge_outranks_recent_volatile_edge() {
    // A day-old, never-decaying structural edge vs a fresh-but-volatile edge
    // that has aged a couple of hours. Effective confidence must keep the
    // structural edge ahead, so beam truncation preserves it.
    let structural = compute_effective_confidence(0.9, REASON_AI_SESSION_PROJECT, 24.0);
    let volatile_old = compute_effective_confidence(0.95, REASON_DOCKER_CONTAINER_ID, 2.0);
    assert!(
        structural > volatile_old,
        "structural {structural} must outrank decayed volatile {volatile_old}"
    );
}

#[test]
fn point_in_time_signal_decays_to_zero_floor() {
    let eff = compute_effective_confidence(0.8, REASON_ERROR_SIGNATURE_MATCH, 10_000.0);
    assert!(eff < 0.01, "error signature decays toward 0: {eff}");
}

#[test]
fn trust_ceiling_caps_correlated_and_zeroes_refuted() {
    use crate::db::graph::{TRUST_CORRELATED, TRUST_REFUTED, TRUST_VERIFIED};
    // Verified passes through unchanged.
    assert!((apply_trust_ceiling(0.9, TRUST_VERIFIED) - 0.9).abs() < 1e-9);
    // Correlated is capped at the ceiling.
    assert!((apply_trust_ceiling(0.9, TRUST_CORRELATED) - TRUST_CORRELATED_CEILING).abs() < 1e-9);
    // A correlated edge already below the ceiling is unchanged.
    assert!((apply_trust_ceiling(0.3, TRUST_CORRELATED) - 0.3).abs() < 1e-9);
    // Refuted contributes nothing.
    assert_eq!(apply_trust_ceiling(0.99, TRUST_REFUTED), 0.0);
}
