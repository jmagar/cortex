use super::*;

#[test]
fn zsh_script_is_emitted_and_calls_complete() {
    let script = zsh_completion_script();
    assert!(script.contains("#compdef cortex"));
    assert!(script.contains("cortex __complete actions"));
    assert!(script.contains("cortex __complete value"));
    assert!(script.contains("cortex __complete flags"));
}

#[test]
fn print_completions_rejects_unsupported_shell() {
    assert!(print_completions("fish").is_err());
    assert!(print_completions("zsh").is_ok());
}
