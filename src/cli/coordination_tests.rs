use super::*;

#[test]
fn doctor_cache_keeps_container_results_keyed_by_name() {
    let mut cache = DoctorCache::default();
    cache.container_inspect.insert(
        "one".to_string(),
        Ok(ContainerMountInfo {
            mount_type: Some("bind".to_string()),
            mount_source: Some("/one".to_string()),
            running: true,
        }),
    );
    cache.container_inspect.insert(
        "two".to_string(),
        Ok(ContainerMountInfo {
            mount_type: Some("volume".to_string()),
            mount_source: Some("/two".to_string()),
            running: false,
        }),
    );

    assert_eq!(
        cache
            .container_inspect("one")
            .unwrap()
            .mount_source
            .as_deref(),
        Some("/one")
    );
    assert_eq!(
        cache
            .container_inspect("two")
            .unwrap()
            .mount_source
            .as_deref(),
        Some("/two")
    );
}

#[test]
fn systemctl_env_parser_keeps_equals_in_values_and_flags_missing_units() {
    let env = parse_systemctl_env_output(
        "Environment=CORTEX_DB_PATH=/data/cortex.db TOKEN=a=b\nLoadState=not-found\n",
    );

    assert_eq!(
        env.inline,
        vec![
            ("CORTEX_DB_PATH".to_string(), "/data/cortex.db".to_string()),
            ("TOKEN".to_string(), "a=b".to_string()),
        ]
    );
    assert!(env.unit_missing);
}
