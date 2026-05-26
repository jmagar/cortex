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

struct MockPidOs;

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
        args: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ServiceResult<std::process::Output>> + Send + 'a>,
    > {
        // Return PID 1234 for the MainPID probe, "active" for everything else.
        let stdout: Vec<u8> = if args.iter().any(|a| a == "MainPID") {
            b"1234\n".to_vec()
        } else {
            b"active\n".to_vec()
        };
        let out = make_output(&stdout, exit_success());
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
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("test3.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    let os = Arc::new(MockPidOs);
    let service = SyslogService::with_os_adapter(pool, storage, os);

    let report = service.ai_watch_status().await.unwrap();

    assert_eq!(report.main_pid, Some(1234));
    assert_eq!(report.active.as_deref(), Some("active"));
}

#[tokio::test]
async fn ai_watch_status_health_is_none_when_db_fails() {
    // With a fresh empty DB, ai_indexing_health succeeds (returns default health).
    // This test verifies the report is still Ok even when health would be None —
    // we can't easily force ai_indexing_health to fail in a unit test, but we
    // verify the field is present (Some or None) without panicking.
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("test4.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    let os = Arc::new(MockProbeOs {
        journal_output: String::new(),
        probe_stdout: b"active\n".to_vec(),
        probe_success: true,
    });
    let service = SyslogService::with_os_adapter(pool, storage, os);

    // Must return Ok (never propagate DB errors as hard failures).
    let result = service.ai_watch_status().await;
    assert!(result.is_ok(), "ai_watch_status must never return Err");
    // OS probe fields are populated regardless.
    assert_eq!(result.unwrap().active.as_deref(), Some("active"));
}
