use super::*;

#[test]
fn service_dependency_key_canonicalizes_mixed_case_slash_input() {
    // Mixed-case `host/service` input must resolve to the lowercase
    // canonical instance key, not pass through verbatim.
    assert_eq!(
        service_dependency_key(None, Some("Tootie/Plex")).unwrap(),
        "tootie/plex"
    );
    // Already-canonical input is unchanged.
    assert_eq!(
        service_dependency_key(None, Some("tootie/plex")).unwrap(),
        "tootie/plex"
    );
    // Plain service + host still combine into the canonical instance key.
    assert_eq!(
        service_dependency_key(Some("Tootie"), Some(" Plex ")).unwrap(),
        "tootie/plex"
    );
}

#[test]
fn service_dependency_key_keeps_legacy_shape_rejection() {
    for legacy in ["tootie:plex", "tootie:plex:plex", "plex/plex/plex"] {
        let err = service_dependency_key(None, Some(legacy)).unwrap_err();
        assert!(
            err.to_string().contains("rejected_legacy_shape"),
            "{legacy}: {err}"
        );
    }
}

#[test]
fn service_dependency_key_rejects_uncanonicalizable_slash_input() {
    // Leading slash: the host component canonicalizes to nothing.
    let err = service_dependency_key(None, Some("/mnt/user")).unwrap_err();
    assert!(err.to_string().contains("does not canonicalize"), "{err}");
}
