use super::*;

#[test]
fn graphql_errors_become_warnings_and_system_normalizes() {
    let mut out = CollectorOutput::new("unraid");
    normalize_section(
        "http://unraid",
        "system",
        &json!({"errors":[{"message":"drift"}],"data":{"info":{"host":"tower","machineId":"abc","os":"unraid"}}}),
        &mut out,
    );
    assert_eq!(out.nodes[0].hostname, "tower");
    assert!(out.errors.iter().any(|e| e.message.contains("errors")));
}

#[test]
fn parses_string_disk_sizes() {
    let mut out = CollectorOutput::new("unraid");
    normalize_section(
        "http://unraid",
        "array",
        &json!({"data":{"array":{"disks":[{"name":"disk1","status":"active","filesystem":"xfs","mountpoint":"/mnt/disk1","size":"42"}]}}}),
        &mut out,
    );
    assert_eq!(out.storage[0].total_bytes, Some(42));
    assert_eq!(out.storage[0].fs_type.as_deref(), Some("xfs"));
    assert_eq!(out.storage[0].mount, "/mnt/disk1");
}

#[test]
fn disk_mapping_falls_back_to_name_and_status() {
    let mut out = CollectorOutput::new("unraid");
    normalize_section(
        "http://unraid",
        "array",
        &json!({"data":{"array":{"disks":[{"name":"disk2","status":"active","size":84}]}}}),
        &mut out,
    );
    assert_eq!(out.storage[0].total_bytes, Some(84));
    assert_eq!(out.storage[0].fs_type.as_deref(), Some("active"));
    assert_eq!(out.storage[0].mount, "disk2");
}

#[tokio::test]
async fn invalid_api_key_header_reports_config_warning() {
    let out = collect(
        Some("http://unraid"),
        Some("bad\nkey"),
        std::time::Duration::from_millis(10),
    )
    .await;
    assert!(out.errors.iter().any(|error| {
        error.phase == "config" && error.message.contains("invalid header characters")
    }));
}
