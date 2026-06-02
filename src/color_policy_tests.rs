//! Tests for color policy resolution.
//!
//! These mutate process-global state (the `COLOR_OVERRIDE` atomic and env
//! vars). nextest isolates each test in its own process, but under
//! `cargo test --lib` (one process, parallel threads — the pre-push path) they
//! would race each other and any other env-reading test, so they are
//! `#[serial]`. Each test installs its own override and clears env up front, so
//! serialized execution is deterministic.

use super::*;
use serial_test::serial;

/// Clear the env vars that influence `resolve` so a test starts from a known
/// baseline regardless of the ambient shell.
fn clear_color_env() {
    for var in ["NO_COLOR", "FORCE_COLOR", "CLICOLOR_FORCE"] {
        unsafe { env::remove_var(var) };
    }
}

#[test]
#[serial]
fn override_always_wins_over_non_tty() {
    clear_color_env();
    install_color_choice(ColorChoice::Always);
    assert!(resolve(false), "Always must force color even on a non-TTY");
}

#[test]
#[serial]
fn override_never_wins_over_tty_and_force() {
    install_color_choice(ColorChoice::Never);
    unsafe { env::set_var("FORCE_COLOR", "1") };
    assert!(
        !resolve(true),
        "Never must suppress color even on a TTY with FORCE_COLOR set"
    );
}

#[test]
#[serial]
fn auto_follows_tty_when_no_env() {
    clear_color_env();
    install_color_choice(ColorChoice::Auto);
    assert!(resolve(true), "Auto + TTY → color");
    assert!(!resolve(false), "Auto + non-TTY → no color");
}

#[test]
#[serial]
fn auto_no_color_env_suppresses_even_on_tty() {
    clear_color_env();
    install_color_choice(ColorChoice::Auto);
    unsafe { env::set_var("NO_COLOR", "1") };
    assert!(!resolve(true), "NO_COLOR must suppress color on a TTY");
}

#[test]
#[serial]
fn auto_force_color_enables_on_non_tty() {
    clear_color_env();
    install_color_choice(ColorChoice::Auto);
    unsafe { env::set_var("FORCE_COLOR", "1") };
    assert!(resolve(false), "FORCE_COLOR must enable color on a non-TTY");
}

#[test]
#[serial]
fn no_color_beats_force_color() {
    clear_color_env();
    install_color_choice(ColorChoice::Auto);
    unsafe {
        env::set_var("NO_COLOR", "1");
        env::set_var("FORCE_COLOR", "1");
    }
    assert!(!resolve(true), "NO_COLOR takes precedence over FORCE_COLOR");
}
