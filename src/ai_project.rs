//! AI project identity normalization shared by transcript ingest paths.

use std::{fs, path::Path};

/// Normalize AI project paths that point at temporary worktrees back to the
/// durable project root used by Cortex's session inventory.
pub(crate) fn normalize_ai_project_path(project: &str) -> String {
    let project = project.trim();
    if project.is_empty() || project.contains("://") {
        return project.to_string();
    }
    if let Some(root) = project_local_worktree_root(project) {
        return root;
    }
    project.to_string()
}

/// Normalize project paths that can be proven against the local filesystem.
///
/// This is intentionally used by the scanner, not the live syslog enrichment
/// path, because Codex app worktree resolution may read a `.git` pointer.
pub(crate) fn normalize_local_ai_project_path(project: &str) -> String {
    let normalized = normalize_ai_project_path(project);
    if normalized != project.trim() {
        return normalized;
    }
    if let Some(root) = codex_app_worktree_project_from_git(project.trim()) {
        return root;
    }
    normalized
}

fn project_local_worktree_root(project: &str) -> Option<String> {
    ["/.worktrees/", "/.claude/worktrees/"]
        .into_iter()
        .find_map(|marker| {
            project
                .find(marker)
                .and_then(|idx| (idx > 0).then(|| project[..idx].to_string()))
        })
}

fn codex_app_worktree_project_from_git(project: &str) -> Option<String> {
    let marker = "/.codex/worktrees/";
    let idx = project.find(marker)?;
    let rest = &project[idx + marker.len()..];
    let mut segments = rest.split('/').filter(|segment| !segment.is_empty());
    let worktree_id = segments.next()?;
    let repo = segments.next()?;
    let worktree = Path::new(&project[..idx])
        .join(".codex/worktrees")
        .join(worktree_id)
        .join(repo);
    let git_pointer = fs::read_to_string(worktree.join(".git")).ok()?;
    let gitdir = git_pointer.trim().strip_prefix("gitdir:")?.trim();
    let gitdir = if Path::new(gitdir).is_absolute() {
        Path::new(gitdir).to_path_buf()
    } else {
        worktree.join(gitdir)
    };
    let gitdir = gitdir.to_string_lossy();
    gitdir
        .find("/.git/worktrees/")
        .and_then(|idx| (idx > 0).then(|| gitdir[..idx].to_string()))
}
