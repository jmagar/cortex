use super::*;

#[test]
fn completes_action_names_with_descriptions() {
    let out = complete(&["actions".into()]).unwrap();
    assert!(out.iter().any(|line| line.starts_with("search\t")));
    assert!(out.iter().any(|line| line.starts_with("tail\t")));
}

#[test]
fn completes_flags_for_action() {
    let out = complete(&["flags".into(), "search".into()]).unwrap();
    assert!(out.iter().any(|l| l.starts_with("--host\t")));
    assert!(out.iter().any(|l| l.starts_with("--since\t")));
    // short alias is offered alongside the long flag
    assert!(out.iter().any(|l| l.starts_with("-n\t")));
}

#[test]
fn completes_static_enum_values_for_severity() {
    let out = complete(&["value".into(), "--severity".into()]).unwrap();
    assert!(out.iter().any(|l| l == "err"));
    assert!(out.iter().any(|l| l == "warning"));
}

#[test]
fn completes_time_hints() {
    let out = complete(&["value".into(), "--since".into()]).unwrap();
    assert!(out.iter().any(|l| l == "1h"));
    assert!(out.iter().any(|l| l == "yesterday"));
}

#[test]
fn dynamic_value_degrades_to_ok_without_db() {
    // Point at a nonexistent DB; host completion must return Ok (empty), never
    // panic or error — completion degrades silently to static candidates.
    let prev = std::env::var_os("CORTEX_DB_PATH");
    unsafe {
        std::env::set_var("CORTEX_DB_PATH", "/nonexistent/cortex-complete-test.db");
    }
    let out = complete(&["value".into(), "--host".into()]);
    unsafe {
        match prev {
            Some(v) => std::env::set_var("CORTEX_DB_PATH", v),
            None => std::env::remove_var("CORTEX_DB_PATH"),
        }
    }
    assert!(out.is_ok(), "dynamic completion must not error: {out:?}");
}

#[test]
fn unknown_context_errors() {
    assert!(complete(&["bogus".into()]).is_err());
}
