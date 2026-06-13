use std::io::{self, ErrorKind};
use std::path::Path;
use std::time::Instant;

use super::{
    DebugComposeAction, DebugWrapperAction, PhaseTimer, SetupPhase, SetupReport, SetupReportInput,
    SetupStatus, check_file_phase, host_local_report_input, setup_path_value, setup_report,
};

pub async fn run_debug_wrapper_setup(action: DebugWrapperAction) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = super::cortex_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let user_home = super::user_home_dir()?;
    let wrapper_path = user_home.join(".local/bin/cortex");
    let repo_path = std::env::current_dir()?;
    let mut phases = Vec::new();

    match action {
        DebugWrapperAction::Install => {
            if let Some(parent) = wrapper_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            write_executable_file(&wrapper_path, &debug_wrapper_script(&repo_path))?;
            phases.push(
                PhaseTimer::start("debug-wrapper")
                    .finish(SetupStatus::Ok, format!("wrote {}", wrapper_path.display())),
            );
        }
        DebugWrapperAction::Remove => match std::fs::remove_file(&wrapper_path) {
            Ok(()) => phases.push(PhaseTimer::start("debug-wrapper").finish(
                SetupStatus::Ok,
                format!("removed {}", wrapper_path.display()),
            )),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                phases.push(PhaseTimer::start("debug-wrapper").finish(
                    SetupStatus::Ok,
                    format!("{} already absent", wrapper_path.display()),
                ));
            }
            Err(error) => return Err(error),
        },
        DebugWrapperAction::Check => {
            phases.push(check_file_phase(
                "debug-wrapper",
                &wrapper_path,
                "run cortex setup debug-wrapper install",
            ));
            phases.push(check_debug_wrapper_content_phase(&wrapper_path, &repo_path));
        }
    }

    let elapsed_ms = started.elapsed().as_millis();
    Ok(setup_report(
        host_local_report_input(
            action.as_str(),
            elapsed_ms,
            home,
            env_path,
            compose_dir,
            data_dir,
        ),
        phases,
    ))
}

pub async fn run_debug_compose_setup(action: DebugComposeAction) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = super::cortex_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let override_path = compose_dir.join("docker-compose.override.yml");
    let repo_path = std::env::current_dir()?;
    let mut phases = Vec::new();

    match action {
        DebugComposeAction::Install => {
            std::fs::create_dir_all(&compose_dir)?;
            write_private_file(&override_path, &debug_compose_override(&repo_path))?;
            phases.push(PhaseTimer::start("debug-compose").finish(
                SetupStatus::Ok,
                format!("wrote {}", override_path.display()),
            ));
        }
        DebugComposeAction::Remove => match std::fs::remove_file(&override_path) {
            Ok(()) => phases.push(PhaseTimer::start("debug-compose").finish(
                SetupStatus::Ok,
                format!("removed {}", override_path.display()),
            )),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                phases.push(PhaseTimer::start("debug-compose").finish(
                    SetupStatus::Ok,
                    format!("{} already absent", override_path.display()),
                ));
            }
            Err(error) => return Err(error),
        },
        DebugComposeAction::Check => {
            phases.push(check_file_phase(
                "debug-compose",
                &override_path,
                "run cortex setup debug-compose install",
            ));
            phases.push(check_debug_compose_content_phase(
                &override_path,
                &repo_path,
            ));
        }
    }

    let elapsed_ms = started.elapsed().as_millis();
    Ok(setup_report(
        SetupReportInput {
            mode: action.as_str(),
            elapsed_ms,
            home,
            env_path,
            compose_dir,
            data_dir,
            health_url: "local debug compose".to_string(),
            mcp_url: "local debug compose".to_string(),
        },
        phases,
    ))
}

pub(crate) fn debug_wrapper_script(repo_path: &Path) -> String {
    let repo_path = setup_path_value(repo_path).expect("validated debug wrapper repo path");
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

repo="${{CORTEX_REPO:-{repo_path}}}"
if [[ ! -d "${{repo}}" ]]; then
  repo="${{HOME}}/workspace/cortex"
fi

cd "${{repo}}"
export CARGO_TARGET_DIR="${{CARGO_TARGET_DIR:-.cache/cargo}}"

case "${{1:-}}" in
  serve|setup)
    ;;
  *)
    export CORTEX_DOCKER_INGEST_ENABLED="${{CORTEX_DOCKER_INGEST_ENABLED:-false}}"
    export CORTEX_AUTH_MODE="${{CORTEX_AUTH_MODE:-bearer}}"
    ;;
esac

cargo build --quiet --bin cortex
exec "${{CARGO_TARGET_DIR}}/debug/cortex" "$@"
"#
    )
}

pub(crate) fn debug_compose_override(repo_path: &Path) -> String {
    let repo_path = setup_path_value(repo_path).expect("validated debug compose repo path");
    format!(
        "services:\n  cortex:\n    image: cortex:local-debug\n    build:\n      context: {repo_path}\n      dockerfile: config/Dockerfile\n      args:\n        CORTEX_BUILD_PROFILE: debug\n"
    )
}

pub(crate) fn check_debug_wrapper_content_phase(
    wrapper_path: &Path,
    repo_path: &Path,
) -> SetupPhase {
    let timer = PhaseTimer::start("debug-wrapper-content");
    let expected = debug_wrapper_script(repo_path);
    match std::fs::read_to_string(wrapper_path) {
        Ok(current) if current == expected => {
            timer.finish(SetupStatus::Ok, "debug wrapper matches generated content")
        }
        Ok(_) => timer.finish(
            SetupStatus::Error,
            format!(
                "{} does not match generated debug wrapper",
                wrapper_path.display()
            ),
        ),
        Err(error) => timer.finish(SetupStatus::Error, error.to_string()),
    }
}

pub(crate) fn check_debug_compose_content_phase(
    override_path: &Path,
    repo_path: &Path,
) -> SetupPhase {
    let timer = PhaseTimer::start("debug-compose-content");
    let expected = debug_compose_override(repo_path);
    match std::fs::read_to_string(override_path) {
        Ok(current) if current == expected => timer.finish(
            SetupStatus::Ok,
            "debug Compose override matches generated content",
        ),
        Ok(_) => timer.finish(
            SetupStatus::Error,
            format!(
                "{} does not match generated debug Compose override",
                override_path.display()
            ),
        ),
        Err(error) => timer.finish(SetupStatus::Error, error.to_string()),
    }
}

// write_executable_file and write_private_file live in parent module (setup.rs).
use super::{write_executable_file, write_private_file};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_wrapper_script_builds_current_repo_debug_binary_with_safe_defaults() {
        let script = debug_wrapper_script(Path::new("/home/jmagar/workspace/cortex"));

        assert!(script.starts_with("#!/usr/bin/env bash\nset -euo pipefail\n"));
        assert!(script.contains(r#"repo="${CORTEX_REPO:-/home/jmagar/workspace/cortex}""#));
        assert!(script.contains(r#"export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-.cache/cargo}""#));
        assert!(script.contains(
            r#"export CORTEX_DOCKER_INGEST_ENABLED="${CORTEX_DOCKER_INGEST_ENABLED:-false}""#
        ));
        assert!(script.contains(r#"export CORTEX_AUTH_MODE="${CORTEX_AUTH_MODE:-bearer}""#));
        assert!(script.contains(r#"exec "${CARGO_TARGET_DIR}/debug/cortex" "$@""#));
    }

    #[test]
    fn debug_wrapper_script_keeps_serve_and_setup_env_unmodified() {
        let script = debug_wrapper_script(Path::new("/home/jmagar/workspace/cortex"));

        assert!(script.contains("case \"${1:-}\" in\n  serve|setup)\n    ;;\n  *)"));
    }

    #[test]
    fn debug_compose_override_builds_debug_image_from_repo_context() {
        let override_yaml = debug_compose_override(Path::new("/home/jmagar/workspace/cortex"));

        assert_eq!(
            override_yaml,
            "services:\n  cortex:\n    image: cortex:local-debug\n    build:\n      context: /home/jmagar/workspace/cortex\n      dockerfile: config/Dockerfile\n      args:\n        CORTEX_BUILD_PROFILE: debug\n"
        );
    }

    #[test]
    fn debug_wrapper_content_phase_reports_match_mismatch_and_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let wrapper_path = tmp.path().join("cortex");
        let repo_path = tmp.path().join("repo");
        std::fs::create_dir(&repo_path).unwrap();
        std::fs::write(&wrapper_path, debug_wrapper_script(&repo_path)).unwrap();

        let ok = check_debug_wrapper_content_phase(&wrapper_path, &repo_path);
        assert_eq!(ok.status, SetupStatus::Ok);
        assert!(ok.detail.contains("matches generated content"));

        std::fs::write(&wrapper_path, "#!/usr/bin/env bash\nexit 0\n").unwrap();
        let stale = check_debug_wrapper_content_phase(&wrapper_path, &repo_path);
        assert_eq!(stale.status, SetupStatus::Error);
        assert!(
            stale
                .detail
                .contains("does not match generated debug wrapper")
        );

        let missing = check_debug_wrapper_content_phase(&tmp.path().join("missing"), &repo_path);
        assert_eq!(missing.status, SetupStatus::Error);
        assert!(missing.detail.contains("No such file") || missing.detail.contains("not found"));
    }

    #[test]
    fn debug_compose_content_phase_reports_match_and_stale_content() {
        let tmp = tempfile::tempdir().unwrap();
        let override_path = tmp.path().join("docker-compose.override.yml");
        let repo_path = tmp.path().join("repo");
        std::fs::create_dir(&repo_path).unwrap();
        std::fs::write(&override_path, debug_compose_override(&repo_path)).unwrap();

        let ok = check_debug_compose_content_phase(&override_path, &repo_path);
        assert_eq!(ok.status, SetupStatus::Ok);
        assert!(ok.detail.contains("matches generated content"));

        std::fs::write(&override_path, "services: {}\n").unwrap();
        let stale = check_debug_compose_content_phase(&override_path, &repo_path);
        assert_eq!(stale.status, SetupStatus::Error);
        assert!(
            stale
                .detail
                .contains("does not match generated debug Compose override")
        );
    }
}
