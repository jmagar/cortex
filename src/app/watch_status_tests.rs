#![cfg(unix)]

use std::sync::Arc;

use crate::app::os_adapter::OsAdapter;
use crate::app::service::SyslogService;
use crate::app::{ServiceError, ServiceResult};
use crate::config::StorageConfig;
use crate::db::init_pool;

fn exit_code(code: u8) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    // from_raw encodes the wait(2) status word: exit code lives in the high byte.
    std::process::ExitStatus::from_raw(i32::from(code) << 8)
}

fn exit_success() -> std::process::ExitStatus {
    exit_code(0)
}

fn make_output(stdout: &[u8], status: std::process::ExitStatus) -> std::process::Output {
    std::process::Output {
        status,
        stdout: stdout.to_vec(),
        stderr: vec![],
    }
}

struct MockProbeOs {
    journal_output: String,
    probe_stdout: Vec<u8>,
    probe_success: bool,
}

impl OsAdapter for MockProbeOs {
    fn run_command<'a>(
        &'a self,
        _program: &'a str,
        _args: &'a [String],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ServiceResult<String>> + Send + 'a>>
    {
        let output = self.journal_output.clone();
        Box::pin(async move { Ok(output) })
    }

    fn probe_command<'a>(
        &'a self,
        _program: &'a str,
        _args: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ServiceResult<std::process::Output>> + Send + 'a>,
    > {
        let status = if self.probe_success {
            exit_success()
        } else {
            exit_code(3)
        };
        let out = make_output(&self.probe_stdout, status);
        Box::pin(async move { Ok(out) })
    }
}

struct MockPidOs {
    pid: u32,
}

impl OsAdapter for MockPidOs {
    fn run_command<'a>(
        &'a self,
        _program: &'a str,
        _args: &'a [String],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ServiceResult<String>> + Send + 'a>>
    {
        Box::pin(async move { Ok(String::new()) })
    }

    fn probe_command<'a>(
        &'a self,
        _program: &'a str,
        _args: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ServiceResult<std::process::Output>> + Send + 'a>,
    > {
        let stdout = format!("{}\n", self.pid).into_bytes();
        Box::pin(async move { Ok(make_output(&stdout, 0)) })
    }
}

fn make_service(os: MockPidOs) -> SyslogService {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("pid_test.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    SyslogService::with_os_adapter(pool, storage, Arc::new(os))
}

struct FailingJournalOs;

impl OsAdapter for FailingJournalOs {
    fn run_command<'a>(
        &'a self,
        _program: &'a str,
        _args: &'a [String],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ServiceResult<String>> + Send + 'a>>
    {
        Box::pin(async move {
            Err(ServiceError::Internal(anyhow::anyhow!(
                "journalctl not found"
            )))
        })
    }

    fn probe_command<'a>(
        &'a self,
        _program: &'a str,
        _args: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ServiceResult<std::process::Output>> + Send + 'a>,
    > {
        // Non-zero exit with non-empty stdout — "inactive" is a valid probe result.
        let out = make_output(b"inactive\n", exit_code(3));
        Box::pin(async move { Ok(out) })
    }
}

#[tokio::test]
async fn ai_watch_status_returns_journal_lines_from_os_adapter() {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("test.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    let os = Arc::new(MockProbeOs {
        journal_output:
            "May 26 10:00:00 host syslog-ai-watch[123]: started\nMay 26 10:01:00 host syslog-ai-watch[123]: indexed 5 files\n"
                .to_string(),
        probe_stdout: b"active\n".to_vec(),
        probe_success: true,
    });
    let service = SyslogService::with_os_adapter(pool, storage, os);

    let report = service.ai_watch_status().await.unwrap();

    assert_eq!(report.service, "syslog-ai-watch.service");
    assert_eq!(report.latest_journal.len(), 2);
    assert!(report.latest_journal[0].contains("started"));
    assert_eq!(report.active.as_deref(), Some("active"));
    assert!(report.journal_error.is_none());
}

#[tokio::test]
async fn ai_watch_status_degrades_gracefully_when_journalctl_fails() {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("test2.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    let os = Arc::new(FailingJournalOs);
    let service = SyslogService::with_os_adapter(pool, storage, os);

    let report = service.ai_watch_status().await.unwrap();

    // journalctl failure degrades to empty vec + journal_error set, not a hard error
    assert!(report.latest_journal.is_empty());
    assert!(
        report.journal_error.is_some(),
        "journal_error should be set when journalctl fails"
    );
    assert_eq!(report.service, "syslog-ai-watch.service");
    // systemctl probe returned "inactive" from the mock (non-zero exit but non-empty stdout)
    assert_eq!(report.active.as_deref(), Some("inactive"));
}

#[tokio::test]
async fn ai_watch_status_parses_main_pid() {
    let service = make_service(MockPidOs { pid: 1234 });
    let report = service.ai_watch_status().await.unwrap();

    assert_eq!(report.main_pid, Some(1234));
}

#[tokio::test]
async fn ai_watch_status_health_is_some_with_valid_db() {
    // Fresh DB → ai_indexing_health succeeds → health is Some.
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("test4.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    let os = Arc::new(MockProbeOs {
        journal_output: String::new(),
        probe_stdout: b"active\n".to_vec(),
        probe_success: true,
    });
    let service = SyslogService::with_os_adapter(pool, storage, os);

    let result = service.ai_watch_status().await;
    assert!(result.is_ok(), "ai_watch_status must never return Err");
    let report = result.unwrap();
    assert!(
        report.health.is_some(),
        "health should be Some with a valid DB"
    );
    assert!(
        report.health_error.is_none(),
        "health_error must be None when health is Some"
    );
    assert_eq!(report.active.as_deref(), Some("active"));
}

#[tokio::test]
async fn ai_watch_status_health_is_none_when_db_schema_broken() {
    // Drop a table that ai_indexing_health queries so it returns Err.
    // ai_watch_status must still return Ok with health=None.
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("test5.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    {
        let conn = pool.get().unwrap();
        conn.execute_batch("DROP TABLE transcript_sources").unwrap();
    }
    let os = Arc::new(MockProbeOs {
        journal_output: String::new(),
        probe_stdout: b"active\n".to_vec(),
        probe_success: true,
    });
    let service = SyslogService::with_os_adapter(pool, storage, os);

    let result = service.ai_watch_status().await;
    assert!(result.is_ok(), "ai_watch_status must never return Err");
    let report = result.unwrap();
    assert!(
        report.health.is_none(),
        "health should be None when DB query fails"
    );
    assert!(
        report.health_error.is_some(),
        "health_error must carry the DB failure message when health is None"
    );
    assert_eq!(
        report.active.as_deref(),
        Some("active"),
        "OS probe fields still populated"
    );
}

#[tokio::test]
async fn ai_watch_status_main_pid_zero_becomes_none() {
    let service = make_service(MockPidOs { pid: 0 });
    let report = service.ai_watch_status().await.unwrap();
    assert_eq!(report.main_pid, None, "PID 0 must be filtered to None");
}
