#[test]
fn ai_search_args_into_request_keeps_filters() {
    let req = crate::cli::AiSearchArgs {
        query: "error".to_string(),
        project: Some("/repo".to_string()),
        tool: Some("codex".to_string()),
        from: Some("2026-01-01T00:00:00Z".to_string()),
        to: None,
        limit: Some(25),
        json: true,
    }
    .into_request();

    assert_eq!(req.query, "error");
    assert_eq!(req.project.as_deref(), Some("/repo"));
    assert_eq!(req.tool.as_deref(), Some("codex"));
    assert_eq!(req.limit, Some(25));
}
