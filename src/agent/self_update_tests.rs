use super::*;

#[test]
fn update_needed_false_for_matching_version() {
    let directive = AgentUpdateDirective {
        version: env!("CARGO_PKG_VERSION").to_string(),
        path: "/v1/agent/binary?os=linux&arch=x86_64".to_string(),
        sha256: "deadbeef".to_string(),
    };
    assert!(!update_needed(&directive));
}

#[test]
fn update_needed_true_for_different_version() {
    let directive = AgentUpdateDirective {
        version: "0.0.0-other".to_string(),
        path: "/v1/agent/binary".to_string(),
        sha256: "deadbeef".to_string(),
    };
    assert!(update_needed(&directive));
}

#[test]
fn sha256_hex_matches_known_vector() {
    // SHA-256 of the empty input.
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    // SHA-256 of "abc".
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn join_url_normalizes_slashes() {
    assert_eq!(
        join_url("http://127.0.0.1:3100", "/v1/agent/binary"),
        "http://127.0.0.1:3100/v1/agent/binary"
    );
    assert_eq!(
        join_url("http://127.0.0.1:3100/", "v1/agent/binary"),
        "http://127.0.0.1:3100/v1/agent/binary"
    );
    assert_eq!(
        join_url(
            "https://cortex.tootie.tv",
            "/v1/agent/binary?os=linux&arch=x86_64"
        ),
        "https://cortex.tootie.tv/v1/agent/binary?os=linux&arch=x86_64"
    );
}

#[test]
fn marker_roundtrips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("cortex");
    std::fs::write(&exe, b"fake").unwrap();

    let marker = UpdateMarker {
        target: "1.2.3".to_string(),
        bak: dir.path().join("cortex.bak-1.1.0"),
        attempts: 2,
    };
    write_marker(&exe, &marker).unwrap();

    let read = read_marker(&marker_path(&exe)).expect("marker present");
    assert_eq!(read.target, "1.2.3");
    assert_eq!(read.attempts, 2);
    assert_eq!(read.bak, marker.bak);
}

#[test]
fn read_marker_absent_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_marker(&dir.path().join("nope.json")).is_none());
}

#[test]
fn backup_current_binary_uses_unique_backup_paths() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("cortex");
    std::fs::write(&exe, b"current").unwrap();
    std::fs::write(dir.path().join("cortex.bak-3.1.0"), b"stale").unwrap();

    let first = backup_current_binary(&exe, dir.path(), "3.1.0").unwrap();
    let second = backup_current_binary(&exe, dir.path(), "3.1.0").unwrap();

    assert_ne!(first, dir.path().join("cortex.bak-3.1.0"));
    assert_ne!(second, dir.path().join("cortex.bak-3.1.0"));
    assert_ne!(first, second);
    assert_eq!(std::fs::read(first).unwrap(), b"current");
    assert_eq!(std::fs::read(second).unwrap(), b"current");
    assert_eq!(
        std::fs::read(dir.path().join("cortex.bak-3.1.0")).unwrap(),
        b"stale"
    );
}

#[test]
fn ensure_binary_still_present_errors_with_clear_diagnosis_when_exe_vanished() {
    // Regression: dookie's agent logged a bare, unhelpful ENOENT ("back up
    // current binary to ...") for hours because a concurrent `cargo build
    // --release` replaced the exact path the running agent was exec'd from
    // (~/.local/bin/cortex was a dev-only symlink into the build output).
    // current_exe() then resolves to "<path> (deleted)", which never exists.
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("cortex (deleted)");

    let err = ensure_binary_still_present(&exe).unwrap_err();
    let message = format!("{err:#}");
    assert!(
        message.contains("no longer exists") && message.contains("concurrent rebuild"),
        "expected a clear diagnosis, got: {message}"
    );
}

#[test]
fn ensure_binary_still_present_ok_when_exe_exists() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("cortex");
    std::fs::write(&exe, b"current").unwrap();
    assert!(ensure_binary_still_present(&exe).is_ok());
}

#[cfg(unix)]
#[test]
fn validate_binary_accepts_matching_version_and_rejects_mismatch() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("fake-cortex");
    std::fs::write(&fake, "#!/bin/sh\necho \"cortex 9.9.9\"\n").unwrap();
    let mut perms = std::fs::metadata(&fake).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake, perms).unwrap();

    assert!(validate_binary(&fake, "9.9.9").is_ok());
    assert!(validate_binary(&fake, "1.2.3").is_err());
}
