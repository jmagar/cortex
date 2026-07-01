#[test]
fn ci_uses_changed_path_classifier_and_stable_gate() {
    let workflow = include_str!("../.github/workflows/ci.yml");

    assert!(
        workflow.contains("scripts/ci/changed_paths.py"),
        "CI must run the changed-path classifier"
    );
    assert!(
        workflow.contains("git show \"${{ github.event.pull_request.base.sha }}:$classifier\""),
        "PRs must use the base-branch classifier when available"
    );

    for job in [
        "version-sync",
        "fmt",
        "clippy",
        "test",
        "coverage",
        "deny",
        "mcp-integration",
    ] {
        let block = workflow_job_block(workflow, job);
        assert!(
            block.contains("needs: [changes"),
            "{job} must depend on the changes job"
        );
        assert!(
            block.contains("needs.changes.outputs."),
            "{job} must be gated by changed-path outputs"
        );
    }

    let gate = workflow_job_block(workflow, "ci-gate");
    for job in [
        "changes",
        "version-sync",
        "fmt",
        "clippy",
        "test",
        "coverage",
        "deny",
        "mcp-integration",
        "gitleaks",
    ] {
        assert!(
            gate.contains(&format!("require_success_or_skipped {job}")),
            "ci-gate must cover {job}"
        );
    }
}

#[test]
fn auto_tag_dispatches_release_as_publish_not_dry_run() {
    let auto_tag = include_str!("../.github/workflows/auto-tag.yml");
    let release = include_str!("../.github/workflows/release.yml");

    assert!(
        auto_tag.contains("gh workflow run release.yml --ref"),
        "auto-tag must explicitly dispatch release.yml after pushing a tag"
    );
    assert!(
        auto_tag.contains("-f publish=true"),
        "auto-tag dispatch must request a real publish; workflow_dispatch defaults to dry-run"
    );
    assert!(
        release.contains("publish:") && release.contains("type: boolean"),
        "release workflow must expose an explicit publish input for workflow_dispatch"
    );
    assert!(
        release.contains("github.event.inputs.publish == 'true'"),
        "release publish job must run for tagged workflow_dispatch when publish=true"
    );
}

#[test]
fn local_cortex_server_has_auto_deploy_timer_contract() {
    let service = include_str!("../config/systemd/cortex-auto-deploy.service");
    let timer = include_str!("../config/systemd/cortex-auto-deploy.timer");
    let script = include_str!("../scripts/auto-deploy.sh");

    assert!(
        service.contains("ExecStart=") && service.contains("scripts/auto-deploy.sh"),
        "auto-deploy service must execute the repo-owned deploy script"
    );
    assert!(
        timer.contains("OnCalendar=") && timer.contains("cortex-auto-deploy.service"),
        "auto-deploy timer must schedule the service"
    );
    for required in [
        "git pull --ff-only",
        "docker compose build cortex",
        "docker compose up -d --no-deps --force-recreate cortex",
        "curl -fsS",
        "docker exec cortex cortex --version",
    ] {
        assert!(
            script.contains(required),
            "auto-deploy script must contain {required:?}"
        );
    }
}

fn workflow_job_block<'a>(workflow: &'a str, job_name: &str) -> &'a str {
    let marker = format!("  {job_name}:");
    let start = workflow
        .find(&marker)
        .unwrap_or_else(|| panic!("job {job_name} exists"));
    let rest = &workflow[start + marker.len()..];
    let end = rest
        .lines()
        .enumerate()
        .skip(1)
        .find_map(|(line_index, line)| {
            let trimmed = line.trim_end();
            if line.starts_with("  ") && !line.starts_with("    ") && trimmed.ends_with(':') {
                let byte_offset = rest
                    .lines()
                    .take(line_index)
                    .map(|line| line.len() + 1)
                    .sum();
                Some(byte_offset)
            } else {
                None
            }
        })
        .unwrap_or(rest.len());
    &rest[..end]
}
