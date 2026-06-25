use std::sync::Arc;

use crate::config::StorageConfig;
use crate::db::{SearchParams, init_pool, search_logs};

use super::*;

fn test_service() -> (CortexService, Arc<DbPool>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("imports-test.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    (CortexService::new(Arc::clone(&pool), storage), pool, dir)
}

#[tokio::test]
async fn import_shell_history_routes_through_service_and_persists_rows() {
    let (service, pool, dir) = test_service();
    let history = dir.path().join(".zsh_history");
    std::fs::write(&history, ": 1716500000:3;cargo test --lib\nplain command\n").unwrap();

    let result = service
        .import_shell_history(history, "zsh".to_string())
        .await
        .unwrap();

    assert_eq!(result.scanned, 2);
    assert_eq!(result.imported, 1);
    assert_eq!(result.skipped, 1);

    let rows = search_logs(
        &pool,
        &SearchParams {
            query: Some("\"cargo test\"".to_string()),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].app_name.as_deref(), Some("zsh"));
}
