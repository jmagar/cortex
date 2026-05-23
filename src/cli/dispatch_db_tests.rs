#[test]
fn db_vacuum_force_false_omits_force_field() {
    let req = crate::cli::DbVacuumArgs {
        full: true,
        pages: 50,
        force: false,
        json: false,
    }
    .into_request();

    assert!(req.full);
    assert_eq!(req.incremental_pages, 50);
    assert_eq!(req.force, None);
}
