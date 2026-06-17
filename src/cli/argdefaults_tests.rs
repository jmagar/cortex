use super::*;

#[test]
fn bind_positional_returns_value_for_action_with_positional() {
    // tail binds a bare positional to --host.
    let bound = positional_value("tail", &["dookie".to_string()]).unwrap();
    assert_eq!(bound.as_deref(), Some("dookie"));
}

#[test]
fn bind_positional_none_when_no_token_given() {
    assert_eq!(positional_value("tail", &[]).unwrap(), None);
}

#[test]
fn bind_positional_errors_when_action_takes_none() {
    // hosts takes no positional; a stray arg is an error.
    let err = positional_value("hosts", &["oops".to_string()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("unexpected"), "{err}");
}

#[test]
fn bind_positional_errors_when_more_than_one_given() {
    let err = positional_value("tail", &["a".to_string(), "b".to_string()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("at most one"), "{err}");
}

#[test]
fn apply_default_limit_only_when_unset() {
    assert_eq!(effective_limit("tail", None), Some(50)); // default applied
    assert_eq!(effective_limit("tail", Some(5)), Some(5)); // user wins
    assert_eq!(effective_limit("status", None), None); // no default
}

#[test]
fn effective_since_resolves_default_window_to_absolute() {
    // errors defaults to a 1h window; absent a user value we get an absolute
    // RFC3339 timestamp (normalised to +00:00 by the time parser).
    let resolved = effective_since("errors", None)
        .unwrap()
        .expect("default applied");
    assert!(resolved.ends_with("+00:00"), "{resolved}");
}

#[test]
fn effective_since_user_value_wins() {
    let user = Some("2026-01-01T00:00:00+00:00".to_string());
    assert_eq!(effective_since("errors", user.clone()).unwrap(), user);
}

#[test]
fn effective_since_none_when_no_default() {
    // tail has no since default.
    assert_eq!(effective_since("tail", None).unwrap(), None);
}
