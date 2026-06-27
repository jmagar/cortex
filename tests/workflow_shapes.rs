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
