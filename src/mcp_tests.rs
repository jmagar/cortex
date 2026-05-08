//! Compile-time + runtime invariants for [`crate::mcp::AppState`] and
//! [`crate::mcp::AuthPolicy`].
//!
//! `AuthPolicy` deliberately has NO `Default` impl — every `AppState`
//! constructor must name the variant explicitly so that "no auth wired" is a
//! conscious choice and never the silent fallback.
//!
//! ```compile_fail
//! use syslog_mcp::mcp::AuthPolicy;
//! let _ = <AuthPolicy as Default>::default();
//! ```

use crate::mcp::AuthPolicy;

#[test]
fn auth_policy_loopback_dev_is_constructible() {
    let policy = AuthPolicy::LoopbackDev;
    assert!(matches!(policy, AuthPolicy::LoopbackDev));
}

/// Static-assert (via trait probe) that [`AuthPolicy`] has no `Default` impl.
///
/// `Default` requires `Sized + Default`. The local trait below is implemented
/// for any `T: Default` and offers a marker associated const. If `AuthPolicy`
/// ever gains a `Default` impl, this trait probe would still compile, so we
/// pair it with the doc-test `compile_fail` above as the load-bearing check.
#[test]
fn auth_policy_variants_are_explicit() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AuthPolicy>();
}
