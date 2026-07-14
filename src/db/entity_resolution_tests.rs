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
