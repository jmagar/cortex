#[test]
fn ingest_rate_by_host_maps_to_optional_true() {
    let req = crate::cli::IngestRateArgs {
        by_host: true,
        json: false,
    }
    .into_request();

    assert_eq!(req.by_host, Some(true));
}
