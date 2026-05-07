//! Constant-time Bearer-token helpers shared by every auth-bearing surface
//! (MCP, non-MCP API, OTLP). Single source of truth for token comparison
//! avoids drift on a security-critical primitive.

use subtle::ConstantTimeEq;

/// Extract the token from an `Authorization: Bearer <token>` header value.
/// Returns `None` if the value is malformed or uses a non-Bearer scheme.
pub(crate) fn bearer_token(auth: &str) -> Option<&str> {
    let mut parts = auth.split_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    if parts.next().is_some() || !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    Some(token)
}

/// Constant-time comparison of two token strings. Both length and bytes are
/// compared in constant time to avoid leaking either via timing.
pub(crate) fn token_matches(provided: &str, expected: &str) -> bool {
    const MAX_TOKEN_LEN: usize = 4096;
    if provided.len() > MAX_TOKEN_LEN || expected.len() > MAX_TOKEN_LEN {
        return false;
    }
    let mut a = [0_u8; MAX_TOKEN_LEN];
    let mut b = [0_u8; MAX_TOKEN_LEN];
    a[..provided.len()].copy_from_slice(provided.as_bytes());
    b[..expected.len()].copy_from_slice(expected.as_bytes());
    let bytes_match = a.ct_eq(&b).unwrap_u8() == 1;
    let lens_match = (provided.len() as u64)
        .ct_eq(&(expected.len() as u64))
        .unwrap_u8()
        == 1;
    bytes_match && lens_match
}
