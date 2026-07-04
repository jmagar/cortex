use std::sync::Arc;

use crate::config::StorageConfig;
use crate::db::{DbPool, init_pool};

use super::*;
use crate::app::models::McpAssessRequest;

fn test_service() -> (CortexService, Arc<DbPool>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("mcp-assess-test.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    (CortexService::new(Arc::clone(&pool), storage), pool, dir)
}

fn base_req() -> McpAssessRequest {
    McpAssessRequest {
        mcp_server: Some("nonexistent-server-xyz".to_string()),
        mcp_tool: None,
        tool_name: None,
        model: None,
        project: None,
        tool: None,
        since: None,
        until: None,
        window_minutes: None,
        correlation_window_minutes: None,
        limit: None,
        all: false,
    }
}

#[tokio::test]
async fn run_mcp_assessment_errors_when_no_target_specified() {
    let (service, _pool, _dir) = test_service();
    let req = McpAssessRequest {
        mcp_server: None,
        mcp_tool: None,
        tool_name: None,
        ..base_req()
    };
    let err = service
        .run_mcp_assessment_with_delta(req, false, |_| Ok(()))
        .await
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("requires an mcp_server, mcp_tool, or tool_name"),
        "unexpected error message: {msg}"
    );
}

#[tokio::test]
async fn run_mcp_assessment_errors_when_no_incident_found() {
    let (service, _pool, _dir) = test_service();
    let req = base_req();
    let err = service
        .run_mcp_assessment_with_delta(req, false, |_| Ok(()))
        .await
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("no MCP incident found") || msg.contains("nonexistent-server-xyz"),
        "unexpected error message: {msg}"
    );
}

#[tokio::test]
async fn run_mcp_assessment_never_touches_gemini_when_run_llm_false() {
    // run_llm=false must skip LlmRunner::run entirely — assert via the
    // absence of any llm_invocations row for action='mcp_assess'.
    let (service, pool, _dir) = test_service();
    let req = base_req();
    let _ = service
        .run_mcp_assessment_with_delta(req, false, |_| Ok(()))
        .await; // Ok(_) or a "no incident found" Err are both fine here.
    let conn = pool.get().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM llm_invocations WHERE action = 'mcp_assess'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "run_llm=false must never invoke LlmRunner::run");
}
