use super::*;

#[test]
fn parses_ahead_and_behind_counts() {
    let (ahead, behind) = parse_ahead_behind("## main...origin/main [ahead 2, behind 3]\n M x");
    assert_eq!(ahead, Some(2));
    assert_eq!(behind, Some(3));
}

#[test]
fn parse_ahead_behind_handles_one_sided_and_plain_branch_status() {
    assert_eq!(
        parse_ahead_behind("## main...origin/main [ahead 7]\n"),
        (Some(7), None)
    );
    assert_eq!(
        parse_ahead_behind("## main...origin/main [behind 4]\n"),
        (None, Some(4))
    );
    assert_eq!(parse_ahead_behind(" M src/lib.rs\n"), (None, None));
}

#[test]
fn discovery_does_not_walk_ignored_dirs() {
    assert!(is_ignored_dir(Path::new("node_modules")));
    assert!(is_ignored_dir(Path::new("target")));
}

#[test]
fn discovery_stops_at_repo_root_and_does_not_report_nested_git_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("nested/.git")).unwrap();

    let mut repos = Vec::new();
    discover_repos(dir.path(), 0, &mut repos);

    assert_eq!(repos, vec![repo]);
}

#[test]
fn discovery_respects_max_depth() {
    let dir = tempfile::tempdir().unwrap();
    let too_deep = dir.path().join("a/b/c/d/repo");
    std::fs::create_dir_all(too_deep.join(".git")).unwrap();

    let mut repos = Vec::new();
    discover_repos(dir.path(), 0, &mut repos);

    assert!(repos.is_empty());
}

#[tokio::test]
async fn collect_warns_when_project_roots_are_empty() {
    let out = collect(&[], std::time::Duration::from_millis(1)).await;

    assert!(out.projects.is_empty());
    assert!(
        out.warnings
            .iter()
            .any(|warning| warning.contains("PROJECT_ROOTS not set"))
    );
}

#[test]
fn discovery_is_sorted_and_skips_symlinked_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let z_repo = dir.path().join("z-repo");
    let a_repo = dir.path().join("a-repo");
    std::fs::create_dir_all(z_repo.join(".git")).unwrap();
    std::fs::create_dir_all(a_repo.join(".git")).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let real = dir.path().join("real");
        std::fs::create_dir_all(real.join(".git")).unwrap();
        symlink(&real, dir.path().join("link-repo")).unwrap();
    }

    let mut repos = Vec::new();
    discover_repos(dir.path(), 0, &mut repos);
    let names = repos
        .iter()
        .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
        .collect::<Vec<_>>();

    #[cfg(unix)]
    assert_eq!(names, vec!["a-repo", "real", "z-repo"]);
    #[cfg(not(unix))]
    assert_eq!(names, vec!["a-repo", "z-repo"]);
    assert!(!names.contains(&"link-repo"));
}
