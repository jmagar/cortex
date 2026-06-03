use chrono::Utc;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::inventory::collectors::CollectorOutput;
use crate::inventory::process::run_command;
use crate::inventory::schema::{ProjectRepo, Provenance};

const MAX_REPOS: usize = 120;
const MAX_DEPTH: usize = 3;

pub async fn collect(roots: &[PathBuf], timeout: Duration) -> CollectorOutput {
    let mut out = CollectorOutput::new("projects");
    if roots.is_empty() {
        out.warn(
            "config",
            "CORTEX_INVENTORY_PROJECT_ROOTS not set; project collection skipped",
        );
        return out;
    }
    let mut repos = Vec::new();
    for root in roots {
        discover_repos(root, 0, &mut repos);
        if repos.len() >= MAX_REPOS {
            out.warn("discovery", "project repo discovery reached max repo cap");
            break;
        }
    }
    for repo in repos.into_iter().take(MAX_REPOS) {
        match repo_summary(&repo, timeout).await {
            Some(summary) => out.projects.push(summary),
            None => out.warn("git", format!("failed to inspect repo {}", repo.display())),
        }
    }
    out
}

fn discover_repos(path: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if out.len() >= MAX_REPOS || depth > MAX_DEPTH {
        return;
    }
    if path.join(".git").exists() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let is_real_dir = entry
            .file_type()
            .map(|kind| kind.is_dir() && !kind.is_symlink())
            .unwrap_or(false);
        if is_real_dir && !is_ignored_dir(&path) {
            discover_repos(&path, depth + 1, out);
        }
    }
}

async fn repo_summary(path: &Path, timeout: Duration) -> Option<ProjectRepo> {
    let path_str = path.to_str()?;
    let branch = git(path_str, &["branch", "--show-current"], timeout).await;
    let head = git(path_str, &["rev-parse", "--short", "HEAD"], timeout).await;
    let status = git(path_str, &["status", "--porcelain=v1", "--branch"], timeout).await?;
    let dirty = status.lines().any(|line| !line.starts_with("##"));
    let (ahead, behind) = parse_ahead_behind(&status);
    let worktrees = git(path_str, &["worktree", "list", "--porcelain"], timeout)
        .await
        .map(|body| {
            body.lines()
                .filter_map(|line| line.strip_prefix("worktree "))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some(ProjectRepo {
        path: path.display().to_string(),
        branch: branch.filter(|s| !s.is_empty()),
        head,
        dirty,
        ahead,
        behind,
        worktrees,
        provenance: Provenance::new("git porcelain", "source_inventory", Utc::now().to_rfc3339()),
    })
}

async fn git(repo: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut full_args = vec!["-C", repo];
    full_args.extend_from_slice(args);
    run_command("git", &full_args, timeout)
        .await
        .ok()
        .filter(|output| output.status == Some(0))
        .map(|output| output.stdout.trim().to_string())
}

fn parse_ahead_behind(status: &str) -> (Option<u32>, Option<u32>) {
    let Some(branch) = status.lines().find(|line| line.starts_with("##")) else {
        return (None, None);
    };
    let ahead = branch
        .split("ahead ")
        .nth(1)
        .and_then(|rest| rest.split([',', ']']).next())
        .and_then(|value| value.parse().ok());
    let behind = branch
        .split("behind ")
        .nth(1)
        .and_then(|rest| rest.split([',', ']']).next())
        .and_then(|value| value.parse().ok());
    (ahead, behind)
}

fn is_ignored_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".cache" | ".git" | "node_modules" | "target"))
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
