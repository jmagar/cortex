use std::sync::Arc;

use crate::app::os_adapter::OsAdapter;
use crate::app::{ServiceError, ServiceResult};
use crate::app::service::SyslogService;
use crate::config::StorageConfig;
use crate::db::init_pool;

fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code)
}

fn make_output(stdout: &[u8], code: i32) -> std::process::Output {
    std::process::Output {
        status: exit_status(code),
        stdout: stdout.to_vec(),
        stderr: vec![],
    }
}

struct MockProbeOs {
    journal_output: String,
    probe_stdout: Vec<u8>,
    probe_exit_code: i32,
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
        let out = make_output(&self.probe_stdout, self.probe_exit_code);
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
            Err(ServiceError::Internal(anyhow::anyhow!("journalctl not found")))
        })
    }

    fn probe_command<'a>(
        &'a self,
        _program: &'a str,
        _args: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ServiceResult<std::process::Output>> + Send + 'a>,
    > {
        Box::pin(async move { Ok(make_output(b"inactive\n", 3)) })
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
        probe_exit_code: 0,
    });
    let service = SyslogService::with_os_adapter(pool, storage, os);

    let report = service.ai_watch_status().await.unwrap();

    assert_eq!(report.service, "syslog-ai-watch.service");
    assert_eq!(report.latest_journal.len(), 2);
    assert!(report.latest_journal[0].contains("started"));
    assert_eq!(report.active.as_deref(), Some("active"));
}

#[tokio::test]
async fn ai_watch_status_degrades_gracefully_when_journalctl_fails() {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("test2.db"));
    let pool = Arc::new(init_pool(&storage).unwrap());
    let os = Arc::new(FailingJournalOs);
    let service = SyslogService::with_os_adapter(pool, storage, os);

    let report = service.ai_watch_status().await.unwrap();

    // journalctl failure degrades to empty vec, not a hard error
    assert!(report.latest_journal.is_empty());
    assert_eq!(report.service, "syslog-ai-watch.service");
    // systemctl probe returned "inactive" from the mock (non-zero exit but non-empty stdout)
    assert_eq!(report.active.as_deref(), Some("inactive"));
}
