use super::vocab::*;

#[test]
fn canonical_service_keys_separate_logic_from_topology() {
    assert_eq!(logical_service_key(" Plex "), Some("plex".to_string()));
    assert_eq!(
        service_instance_key("Tootie", " Plex "),
        Some("tootie/plex".to_string())
    );
    assert_eq!(
        split_service_instance_key("tootie/plex"),
        Some(("tootie", "plex"))
    );
}

#[test]
fn old_nested_service_shapes_are_classified_not_normalized() {
    assert_eq!(
        classify_legacy_shape("tootie:plex"),
        Some(LegacyShape::HostService)
    );
    assert_eq!(
        classify_legacy_shape("tootie:plex:plex"),
        Some(LegacyShape::HostProjectService)
    );
    assert_eq!(
        classify_legacy_shape("plex/plex/plex"),
        Some(LegacyShape::SlashTriplet)
    );
    assert_eq!(classify_legacy_shape("plex"), None);
    assert_eq!(classify_legacy_shape("tootie/plex"), None);
}

use super::adapters::*;
use super::observation::*;

#[test]
fn agent_docker_identity_extracts_structured_service_instance() {
    let identity = AgentDockerIdentity {
        agent_host: "Tootie".to_string(),
        container_id: "abcdef1234567890".to_string(),
        container_name: "plex".to_string(),
        compose_project: Some("plex".to_string()),
        compose_service: Some("plex".to_string()),
        image: Some("lscr.io/linuxserver/plex:latest".to_string()),
        stream: "stdout".to_string(),
        observed_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let observations = observations_from_agent_docker_identity(&identity);
    assert!(observations.iter().any(|o| {
        o.kind == ObservationKind::ServiceInstance
            && o.service_instance_key.as_deref() == Some("tootie/plex")
            && o.logical_service_key.as_deref() == Some("plex")
            && o.trust == ResolverTrust::Verified
            && o.structured
    }));
}

#[test]
fn raw_app_label_does_not_create_logical_service_observation_by_itself() {
    let observations = observations_from_raw_app_label(
        "plex/plex/plex",
        "tootie",
        "log",
        "42",
        "2026-01-01T00:00:00Z",
    );
    assert!(
        observations
            .iter()
            .any(|o| o.kind == ObservationKind::RawAppLabel)
    );
    assert!(
        !observations
            .iter()
            .any(|o| o.kind == ObservationKind::LogicalService)
    );
}

#[test]
fn safe_observation_display_redacts_sensitive_values() {
    assert_eq!(
        safe_display_value("https://user:pass@example.test/path"),
        "[redacted]"
    );
    assert_eq!(
        safe_display_value("/home/jmagar/.cortex/token.txt"),
        "[redacted]"
    );
    assert_eq!(safe_display_value("plex"), "plex");
}

use super::resolver::*;

#[test]
fn resolver_converges_duplicate_hosts_under_one_logical_service() {
    let tootie = ResolverObservation {
        kind: ObservationKind::ServiceInstance,
        observed_key: "tootie/plex".to_string(),
        display_label: "tootie/plex".to_string(),
        host_key: Some("tootie".to_string()),
        logical_service_key: Some("plex".to_string()),
        service_instance_key: Some("tootie/plex".to_string()),
        source_kind: "app_inventory".to_string(),
        source_id: "inventory:tootie".to_string(),
        evidence_path: "inventory.services.plex".to_string(),
        observed_at: "2026-01-01T00:00:00Z".to_string(),
        trust: ResolverTrust::Verified,
        structured: true,
    };
    let shart = ResolverObservation {
        service_instance_key: Some("shart/plex".to_string()),
        host_key: Some("shart".to_string()),
        source_id: "inventory:shart".to_string(),
        observed_key: "shart/plex".to_string(),
        display_label: "shart/plex".to_string(),
        ..tootie.clone()
    };
    let decisions = resolve_observations(&[tootie, shart]);
    assert!(
        decisions
            .iter()
            .any(|d| { d.entity_type == ENTITY_TYPE_LOGICAL_SERVICE && d.canonical_key == "plex" })
    );
    assert!(decisions.iter().any(|d| {
        d.entity_type == ENTITY_TYPE_SERVICE_INSTANCE && d.canonical_key == "tootie/plex"
    }));
    assert!(decisions.iter().any(|d| {
        d.entity_type == ENTITY_TYPE_SERVICE_INSTANCE && d.canonical_key == "shart/plex"
    }));
}

#[test]
fn resolver_rejects_old_key_shapes_before_lookup() {
    for input in ["tootie:plex", "tootie:plex:plex", "plex/plex/plex"] {
        let diagnostic = diagnose_lookup_input(input);
        assert_eq!(diagnostic.status, ResolverStatus::RejectedLegacyShape);
        assert_eq!(diagnostic.reason, "rejected_legacy_shape");
        assert!(diagnostic.candidates.is_empty());
    }
}

#[test]
fn weak_raw_labels_do_not_upgrade_themselves() {
    let observations =
        observations_from_raw_app_label("complex", "tootie", "log", "99", "2026-01-01T00:00:00Z");
    let decisions = resolve_observations(&observations);
    assert!(!decisions.iter().any(|d| d.canonical_key == "plex"));
    assert!(
        !decisions
            .iter()
            .any(|d| d.entity_type == ENTITY_TYPE_LOGICAL_SERVICE)
    );
}

#[test]
fn structured_agent_docker_metadata_resolves_without_central_docker_uri() {
    let identity = AgentDockerIdentity {
        agent_host: "tootie".to_string(),
        container_id: "abcdef1234567890".to_string(),
        container_name: "plex".to_string(),
        compose_project: Some("plex".to_string()),
        compose_service: Some("plex".to_string()),
        image: Some("lscr.io/linuxserver/plex:latest".to_string()),
        stream: "stdout".to_string(),
        observed_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let observations = observations_from_agent_docker_identity(&identity);
    let decisions = resolve_observations(&observations);
    assert!(decisions.iter().any(|d| {
        d.entity_type == ENTITY_TYPE_SERVICE_INSTANCE && d.canonical_key == "tootie/plex"
    }));
}
