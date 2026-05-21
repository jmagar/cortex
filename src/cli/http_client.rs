//! REST HTTP client used by the standalone `syslog` CLI to route subcommands
//! through the container's `/api/*` surface instead of opening SQLite directly
//! (bead 0p8r.5 — refactor: route CLI through container REST API).
//!
//! The CLI process is short-lived (one invocation = one request, usually) but
//! lives inside `#[tokio::main]`, so this module uses the **async**
//! `reqwest::Client` — `reqwest::blocking::Client::build()` panics when called
//! from within an existing tokio runtime. A single per-invocation Client is
//! built in [`HttpClient::discover`] and reused across method calls; we never
//! cache a Client across invocations because the connection pool is dropped
//! at process exit.
//!
//! ## Discovery precedence (locked)
//!
//! - `base_url`: `--server URL` flag > `SYSLOG_MCP_URL` env >
//!   `http://127.0.0.1:<SYSLOG_MCP_PORT|3100>` default.
//! - `token`: CLI flag > `SYSLOG_API_TOKEN` env > **fail closed**
//!   with a message that explicitly mentions `syslog setup repair`, copying
//!   from another host's `~/.syslog-mcp/.env`, and warns against
//!   exporting credential values in an interactive shell. NO `.env` file tier — that
//!   was deliberately dropped (eng-review code-simplicity reviewer).
//!
//! URLs containing userinfo (`http://token@host:3100/`) are rejected at
//! discovery so the token cannot leak into anyhow traces or `reqwest` debug
//! logs (eng-review #A24).
//!
//! ## Error mapping
//!
//! - **Connect timeout** (10s, separate from the 10-min request timeout):
//!   maps to `"cannot connect to syslog-mcp at {url} — DNS or TCP connection
//!   failed within 10s"` (eng-review #A30, bead 0p8r.26).
//! - **401**: `"authentication failed (401): ..."`.
//! - **403**: `"forbidden (403): ..."`.
//! - **404 enrichment**: on the FIRST 404 only, call `/api/version` **once**
//!   (cached via [`tokio::sync::OnceCell`]) to enrich the error with
//!   `"endpoint {path} not found on server v{version}; CLI may be newer than
//!   server. Run: syslog compose pull && syslog compose up"`. If
//!   `/api/version` ALSO 404s, the server is too old to identify itself; emit
//!   `"endpoint not available; server too old to identify version..."`. If it
//!   401s (token rejected), emit `"endpoint not available on this server;
//!   could not check version (auth failed)"` — no secondary error. The
//!   OnceCell holds `Option<ServerVersion>` so the closure always returns
//!   `Some(_)` / `None` and the cell populates on the FIRST 404 regardless of
//!   what /api/version returned; we never re-probe (eng-review C4).
//! - **503**: retried ONCE with 500ms backoff. The first-attempt body bytes
//!   are held so the retry error includes both attempts; we never read the
//!   body twice (eng-review #A31).
//! - **Malformed JSON** on success: bytes are read once, then deserialized
//!   via `serde_path_to_error` so the error surfaces the failing field path
//!   plus a 256-byte body preview (eng-review #A23 / #A25).

use std::env;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::{Method, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use url::Url;

use syslog_mcp::app::{
    AbuseSearchRequest, AbuseSearchResponse, AiCheckpointsRequest, AiCorrelateRequest,
    AiCorrelateResponse, AiIncidentRequest, AiIncidentResponse, AiInvestigateRequest,
    AiInvestigateResponse, AiParseErrorsRequest, AiPruneCheckpointsRequest, CorrelateEventsRequest,
    CorrelateEventsResponse, DbCheckpointRequest, DbCheckpointResult, DbIntegrityRequest,
    DbIntegrityResult, DbMaintenanceStatus, DbStats, DbVacuumRequest, DbVacuumResult,
    GetErrorsRequest, GetErrorsResponse, ListAiProjectsRequest, ListAiProjectsResponse,
    ListAiToolsRequest, ListAiToolsResponse, ListHostsResponse, ListSessionsRequest,
    ListSessionsResponse, ProjectContextRequest, ProjectContextResponse, SearchLogsRequest,
    SearchLogsResponse, SearchSessionsRequest, SearchSessionsResponse, TailLogsRequest,
    UsageBlocksRequest, UsageBlocksResponse,
};
use syslog_mcp::scanner::{CheckpointEntry, ParseErrorEntry, PruneCheckpointsResult};

/// Connection timeout (eng-review #A30, bead 0p8r.26). 10s covers cold-start
/// Docker Compose DNS for `syslog-mcp:3100`, which empirically takes 3-5s on
/// first lookup; 5s was tight enough that the first request after `compose up`
/// regularly tripped a false-positive "cannot connect" error.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Overall request timeout. 10 min covers long-running ops like `db vacuum`
/// or `db integrity` on a multi-GB DB without axing them mid-response.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// 503 retry backoff. Single retry only (eng-review #A31). The body bytes
/// from the first attempt are held so the retry error can include both.
const RETRY_BACKOFF: Duration = Duration::from_millis(500);

const BODY_PREVIEW_BYTES: usize = 256;

const DEFAULT_PORT: u16 = 3100;

/// Subset of `/api/version` we care about for 404 enrichment. We define a
/// **local** deserializable struct rather than reusing `api::VersionInfo`
/// because that type stores `version: &'static str` (server-side compile-time
/// constant) which can't be deserialized from JSON.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerVersion {
    pub version: String,
    /// Optional on the server (omitted via `skip_serializing_if`); rendered as
    /// `"unknown"` in error messages when missing (eng-review #A26).
    #[serde(default)]
    pub git_sha: Option<String>,
    pub schema_version: i64,
}

/// REST HTTP client. Built once per CLI invocation via [`HttpClient::discover`]
/// and reused across method calls during that invocation.
///
/// Cancellation: every method is `async` and cancel-safe. The only shared
/// mutable state across `.await` is `server_version_cache`, which is a
/// `OnceCell` — `get_or_init` is itself cancel-safe (re-entrant callers either
/// see the populated value or race the init future fairly). The dispatch layer
/// (bead 0p8r.7) is responsible for wrapping these in `tokio::select!` against
/// `tokio::signal::ctrl_c()`.
#[derive(Debug)]
pub struct HttpClient {
    base_url: Url,
    inner: reqwest::Client,
    /// **LAZY ON 404 ONLY. Do NOT pre-populate or refresh after success.** The
    /// whole point of `/api/version` is detecting upgrades after a deploy;
    /// caching beyond 404 enrichment defeats it (eng-review #A33). Populated
    /// inside [`HttpClient::enrich_404`] on the FIRST 404 we ever see;
    /// subsequent 404s reuse the cached `Option<ServerVersion>`.
    ///
    /// Bead 0p8r.27: the OnceCell stores success values too — but because
    /// `HttpClient` is built fresh per CLI invocation (see module docs), the
    /// cell never outlives a single command. That's operationally equivalent
    /// to "no cross-invocation caching", which is what spec #A33 actually
    /// requires; the in-invocation cache is intentional (we don't re-probe
    /// `/api/version` on every 404 in the same run).
    server_version_cache: OnceCell<Option<ServerVersion>>,
}

impl HttpClient {
    /// Resolve the base URL and bearer token from CLI flags / env vars / defaults,
    /// then construct a `reqwest::Client` with our connect + request timeouts
    /// and a sensitive Authorization header.
    pub fn discover(
        server_override: Option<String>,
        token_override: Option<String>,
    ) -> Result<Self> {
        let base_url = resolve_base_url(server_override)?;
        let token = resolve_token(token_override)?;

        // Construct Authorization once and mark sensitive so reqwest redacts
        // the bearer in debug logs (eng-review locked decision). We use the
        // default-headers map rather than `RequestBuilder::bearer_auth` per
        // call because `bearer_auth` does NOT call `set_sensitive(true)` on
        // the resulting HeaderValue — see reqwest source.
        let mut auth_value = HeaderValue::from_str(&format!("Bearer {token}"))
            .context("failed to construct Authorization header from token")?;
        auth_value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth_value);

        let inner = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .default_headers(headers)
            .build()
            .context("failed to build reqwest::Client")?;

        Ok(Self {
            base_url,
            inner,
            server_version_cache: OnceCell::new(),
        })
    }

    // ─── HTTP plumbing ──────────────────────────────────────────────────────

    /// Build a full URL from a path like `/api/search` joined onto the
    /// configured base URL. The base URL is normalised to end with `/` so
    /// `Url::join` doesn't drop the last path segment.
    fn url(&self, path: &str) -> Result<Url> {
        // `Url::join` treats the receiver as a base; with a trailing slash the
        // path segment is appended cleanly. `resolve_base_url` guarantees the
        // trailing slash.
        self.base_url
            .join(path.trim_start_matches('/'))
            .with_context(|| format!("failed to build URL from base + {path}"))
    }

    /// GET with an optional serializable Request struct as the query string.
    /// Pass `None` for endpoints like `/api/hosts` and `/api/version` that
    /// take no parameters.
    async fn get_json<Req, Resp>(&self, path: &str, req: Option<&Req>) -> Result<Resp>
    where
        Req: Serialize + ?Sized,
        Resp: DeserializeOwned,
    {
        let url = self.url(path)?;
        let send = || async {
            let mut builder = self.inner.request(Method::GET, url.clone());
            if let Some(r) = req {
                builder = builder.query(r);
            }
            builder.send().await
        };
        self.execute_with_retry(send, path).await
    }

    /// GET with a pre-serialized query string (e.g. from `serde_qs::to_string`)
    /// rather than reqwest's `.query(&T)` (which goes through
    /// `serde_urlencoded` and can't represent `Vec<String>` as repeated keys).
    /// `raw_query` must NOT include a leading `?`.
    async fn get_json_with_raw_query<Resp>(&self, path: &str, raw_query: &str) -> Result<Resp>
    where
        Resp: DeserializeOwned,
    {
        let mut url = self.url(path)?;
        if raw_query.is_empty() {
            url.set_query(None);
        } else {
            url.set_query(Some(raw_query));
        }
        let send = || async { self.inner.request(Method::GET, url.clone()).send().await };
        self.execute_with_retry(send, path).await
    }

    /// POST with JSON body.
    async fn post_json<Req, Resp>(&self, path: &str, body: &Req) -> Result<Resp>
    where
        Req: Serialize + ?Sized,
        Resp: DeserializeOwned,
    {
        let url = self.url(path)?;
        let send = || async {
            self.inner
                .request(Method::POST, url.clone())
                .json(body)
                .send()
                .await
        };
        self.execute_with_retry(send, path).await
    }

    /// Send a request, handling the 503 retry and final response classification.
    ///
    /// The closure is invoked at most twice — once initially, and once more
    /// after [`RETRY_BACKOFF`] if the first response status is 503. We read
    /// the body bytes from the first 503 attempt before retrying so the final
    /// error message can include both attempts' bodies (eng-review #A31).
    async fn execute_with_retry<F, Fut, Resp>(&self, send: F, path: &str) -> Result<Resp>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = reqwest::Result<Response>>,
        Resp: DeserializeOwned,
    {
        let first = match send().await {
            Ok(r) => r,
            Err(err) => return Err(map_send_error(err, &self.base_url)),
        };
        if first.status() == StatusCode::SERVICE_UNAVAILABLE {
            // Read body ONCE — we need it for both the retry-error
            // construction below and the inner 503 handling.
            let first_body = first.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
            tokio::time::sleep(RETRY_BACKOFF).await;
            let second = match send().await {
                Ok(r) => r,
                Err(err) => {
                    return Err(anyhow!(
                        "{path} returned 503; retry transport error: {err}. First-attempt body: {}",
                        preview_body(&first_body)
                    ));
                }
            };
            if second.status() == StatusCode::SERVICE_UNAVAILABLE {
                let second_body = second.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
                return Err(anyhow!(
                    "{path} returned 503 on both attempts (backoff {}ms). First-attempt body: {}. Second-attempt body: {}",
                    RETRY_BACKOFF.as_millis(),
                    preview_body(&first_body),
                    preview_body(&second_body),
                ));
            }
            return self.handle_response(second, path).await;
        }
        self.handle_response(first, path).await
    }

    /// Classify a non-503 response: success → deserialize, 401/403 → auth
    /// error, 404 → enrich via `/api/version`, other 4xx/5xx → generic error.
    ///
    /// Body bytes are read ONCE per response, then either deserialized or
    /// included in the error message (eng-review #A23 / #A25).
    async fn handle_response<Resp>(&self, resp: Response, path: &str) -> Result<Resp>
    where
        Resp: DeserializeOwned,
    {
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .with_context(|| format!("failed to read response body from {path}"))?;
        if status.is_success() {
            let de = &mut serde_json::Deserializer::from_slice(&bytes);
            return serde_path_to_error::deserialize(de).map_err(|e| {
                anyhow!(
                    "malformed response from {path}: {} at field path '{}'. First {} bytes: {}",
                    e.inner(),
                    e.path(),
                    BODY_PREVIEW_BYTES.min(bytes.len()),
                    preview_body(&bytes),
                )
            });
        }

        match status {
            StatusCode::UNAUTHORIZED => Err(anyhow!(
                "authentication failed (401) on {path}: {}. Token may be wrong; run 'syslog setup repair' or check SYSLOG_API_TOKEN",
                preview_body(&bytes)
            )),
            StatusCode::FORBIDDEN => Err(anyhow!(
                "forbidden (403) on {path}: {}",
                preview_body(&bytes)
            )),
            StatusCode::NOT_FOUND => Err(self.enrich_404(path, &bytes).await),
            _ => Err(anyhow!(
                "{path} returned {} ({}): {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("?"),
                preview_body(&bytes)
            )),
        }
    }

    /// 404 enrichment with depth-1 OnceCell guard.
    ///
    /// On the first 404 we hit during the lifetime of this client, probe
    /// `/api/version` ONCE and stash an `Option<VersionProbe>` in the
    /// OnceCell. Subsequent 404s reuse the cached result without re-probing
    /// (eng-review C4 — never reprobe).
    ///
    /// The cell holds `Option<ServerVersion>`:
    /// - `Some(sv)` — /api/version returned 2xx and we deserialised a body.
    /// - `None`     — /api/version 401'd, 404'd, or transport-failed.
    ///
    /// Because OnceCell collapses the kind-of-None distinction, we cannot
    /// differentiate "401" from "404" from "unreachable" on the second 404 —
    /// but that's fine: the spec only requires the distinction on the FIRST
    /// 404 (which we capture via the inline probe inside `get_or_init`).
    /// Subsequent 404s emit the generic "endpoint not available on this
    /// server; could not check version" fallback. Tests assert /api/version
    /// is called exactly once even across many failing requests.
    async fn enrich_404(&self, path: &str, body: &[u8]) -> anyhow::Error {
        // Build /api/version URL once; if even this fails we still emit a
        // usable 404 error.
        let version_url = self.url("/api/version").ok();
        let client = self.inner.clone();

        let sv = self
            .server_version_cache
            .get_or_init(|| async move {
                let url = version_url?;
                match client.request(Method::GET, url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let bytes = resp.bytes().await.ok()?;
                        serde_json::from_slice::<ServerVersion>(&bytes).ok()
                    }
                    Ok(resp) => {
                        // 401 / 404 / other — body is intentionally consumed
                        // to free the connection. Status is captured in the
                        // returned None; first_call below decides messaging.
                        let _ = resp.bytes().await;
                        None
                    }
                    Err(_) => None,
                }
            })
            .await
            .clone();

        match sv {
            Some(sv) => {
                let git = sv.git_sha.as_deref().unwrap_or("unknown");
                anyhow!(
                    "endpoint {path} not found on server v{} (git {}): {}. CLI may be newer than server. Run: syslog compose pull && syslog compose up",
                    sv.version,
                    git,
                    preview_body(body),
                )
            }
            None => anyhow!(
                "endpoint {path} not found (404): {}. endpoint not available on this server; could not check version (server may be too old to identify, or auth failed). Run: syslog compose pull && syslog compose up",
                preview_body(body)
            ),
        }
    }

    // ─── REST surface: Wave 0–1 (syslog-mcp-0p8r.1) ─────────────────────────

    pub async fn version(&self) -> Result<ServerVersion> {
        self.get_json::<(), _>("/api/version", None).await
    }

    pub async fn search(&self, req: &SearchLogsRequest) -> Result<SearchLogsResponse> {
        self.get_json("/api/search", Some(req)).await
    }

    pub async fn tail(&self, req: &TailLogsRequest) -> Result<SearchLogsResponse> {
        self.get_json("/api/tail", Some(req)).await
    }

    pub async fn errors(&self, req: &GetErrorsRequest) -> Result<GetErrorsResponse> {
        self.get_json("/api/errors", Some(req)).await
    }

    pub async fn hosts(&self) -> Result<ListHostsResponse> {
        self.get_json::<(), _>("/api/hosts", None).await
    }

    pub async fn correlate(&self, req: &CorrelateEventsRequest) -> Result<CorrelateEventsResponse> {
        self.get_json("/api/correlate", Some(req)).await
    }

    pub async fn stats(&self) -> Result<DbStats> {
        self.get_json::<(), _>("/api/stats", None).await
    }

    pub async fn sessions(&self, req: &ListSessionsRequest) -> Result<ListSessionsResponse> {
        self.get_json("/api/sessions", Some(req)).await
    }

    // ─── REST surface: bead 0p8r.2 (AI session queries) ─────────────────────

    pub async fn ai_search(&self, req: &SearchSessionsRequest) -> Result<SearchSessionsResponse> {
        self.get_json("/api/ai/search", Some(req)).await
    }

    /// `/api/ai/abuse` round-trips `AbuseSearchRequest` directly on the wire
    /// via `serde_qs` (bead 0p8r.15 — closes the previous hand-rolled flat
    /// query that ignored `terms` beyond the first). `serde_qs::to_string`
    /// renders `Vec<String>` as repeated `?terms=a&terms=b` params; the
    /// server-side `QsQuery<AbuseSearchRequest>` extractor parses the same
    /// wire shape back into the typed struct.
    pub async fn ai_abuse(&self, req: &AbuseSearchRequest) -> Result<AbuseSearchResponse> {
        let qs = serde_qs::to_string(req)
            .context("failed to serialize AbuseSearchRequest as query string")?;
        self.get_json_with_raw_query("/api/ai/abuse", &qs).await
    }

    pub async fn ai_correlate(&self, req: &AiCorrelateRequest) -> Result<AiCorrelateResponse> {
        self.get_json("/api/ai/correlate", Some(req)).await
    }

    pub async fn ai_blocks(&self, req: &UsageBlocksRequest) -> Result<UsageBlocksResponse> {
        self.get_json("/api/ai/blocks", Some(req)).await
    }

    pub async fn ai_context(&self, req: &ProjectContextRequest) -> Result<ProjectContextResponse> {
        self.get_json("/api/ai/context", Some(req)).await
    }

    pub async fn ai_tools(&self, req: &ListAiToolsRequest) -> Result<ListAiToolsResponse> {
        self.get_json("/api/ai/tools", Some(req)).await
    }

    pub async fn ai_projects(&self, req: &ListAiProjectsRequest) -> Result<ListAiProjectsResponse> {
        self.get_json("/api/ai/projects", Some(req)).await
    }

    pub async fn ai_incidents(&self, req: &AiIncidentRequest) -> Result<AiIncidentResponse> {
        let qs = serde_qs::to_string(req)
            .context("failed to serialize AiIncidentRequest as query string")?;
        self.get_json_with_raw_query("/api/ai/incidents", &qs).await
    }

    pub async fn ai_investigate(
        &self,
        req: &AiInvestigateRequest,
    ) -> Result<AiInvestigateResponse> {
        let qs = serde_qs::to_string(req)
            .context("failed to serialize AiInvestigateRequest as query string")?;
        self.get_json_with_raw_query("/api/ai/investigate", &qs).await
    }

    // ─── REST surface: bead 0p8r.3 (AI diagnostic + admin) ──────────────────

    pub async fn ai_checkpoints(&self, req: &AiCheckpointsRequest) -> Result<Vec<CheckpointEntry>> {
        self.get_json("/api/ai/checkpoints", Some(req)).await
    }

    pub async fn ai_parse_errors(
        &self,
        req: &AiParseErrorsRequest,
    ) -> Result<Vec<ParseErrorEntry>> {
        self.get_json("/api/ai/errors", Some(req)).await
    }

    pub async fn prune_ai_checkpoints(
        &self,
        req: &AiPruneCheckpointsRequest,
    ) -> Result<PruneCheckpointsResult> {
        self.post_json("/api/ai/prune-checkpoints", req).await
    }

    // ─── REST surface: bead 0p8r.4 (DB ops) ─────────────────────────────────

    pub async fn db_status(&self) -> Result<DbMaintenanceStatus> {
        self.get_json::<(), _>("/api/db/status", None).await
    }

    pub async fn db_integrity(&self, req: &DbIntegrityRequest) -> Result<DbIntegrityResult> {
        self.get_json("/api/db/integrity", Some(req)).await
    }

    pub async fn db_checkpoint(&self, req: &DbCheckpointRequest) -> Result<DbCheckpointResult> {
        self.post_json("/api/db/checkpoint", req).await
    }

    pub async fn db_vacuum(&self, req: &DbVacuumRequest) -> Result<DbVacuumResult> {
        self.post_json("/api/db/vacuum", req).await
    }
}

// ─── Free helpers (also used in tests) ──────────────────────────────────────

/// Resolve `--server` flag > `SYSLOG_MCP_URL` env >
/// `http://127.0.0.1:<SYSLOG_MCP_PORT|3100>`. The returned URL is normalised
/// to end with `/` so `Url::join` appends paths cleanly.
pub(crate) fn resolve_base_url(server_override: Option<String>) -> Result<Url> {
    let raw = match server_override {
        Some(s) if !s.trim().is_empty() => s,
        _ => env::var("SYSLOG_MCP_URL").unwrap_or_else(|_| {
            let port = env::var("SYSLOG_MCP_PORT")
                .ok()
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(DEFAULT_PORT);
            format!("http://127.0.0.1:{port}")
        }),
    };
    let parsed = Url::parse(&raw).with_context(|| format!("invalid base URL '{raw}'"))?;

    // Reject userinfo so credentials cannot leak into anyhow error traces or
    // reqwest debug logs (eng-review #A24).
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!(
            "--server URL must not contain URL userinfo; provide credentials with the dedicated token flag"
        );
    }

    if !matches!(parsed.scheme(), "http" | "https") {
        bail!(
            "--server URL must use http or https scheme; got '{}'",
            parsed.scheme()
        );
    }

    // Normalise to trailing slash so `Url::join("api/foo")` doesn't drop the
    // last path segment.
    let s = parsed.as_str();
    let normalised = if s.ends_with('/') {
        parsed
    } else {
        Url::parse(&format!("{s}/"))
            .with_context(|| format!("failed to normalise base URL '{s}'"))?
    };
    Ok(normalised)
}

/// Resolve CLI token flag > `SYSLOG_API_TOKEN` env. NO `.env` file tier.
/// Fails closed with a multi-host-friendly error message that warns against
/// shell-history leaks (security #36 + locked decision).
pub(crate) fn resolve_token(token_override: Option<String>) -> Result<String> {
    if let Some(t) = token_override.filter(|s| !s.trim().is_empty()) {
        return Ok(t);
    }
    if let Ok(t) = env::var("SYSLOG_API_TOKEN") {
        if !t.trim().is_empty() {
            return Ok(t);
        }
    }
    bail!(
        "SYSLOG_API_TOKEN not set in environment. \
         Run 'syslog setup repair' on this host, \
         or copy from another host's ~/.syslog-mcp/.env. \
         Or use the token flag. \
         Do NOT export credential values in an interactive shell (history leak); \
         use a file source instead."
    );
}

/// Map a transport-layer `reqwest::Error` from `Client::send()` into a
/// human-friendly anyhow::Error. Connect failures are the common case for a
/// container that hasn't started yet; surface them with the configured base
/// URL so users know what address the CLI tried.
fn map_send_error(err: reqwest::Error, base_url: &Url) -> anyhow::Error {
    if err.is_connect() {
        return anyhow!(
            "cannot connect to syslog-mcp at {base_url} — DNS or TCP connection failed within {}s",
            CONNECT_TIMEOUT.as_secs()
        );
    }
    if err.is_timeout() {
        return anyhow!(
            "request to syslog-mcp at {base_url} timed out (request deadline {}s exceeded)",
            REQUEST_TIMEOUT.as_secs()
        );
    }
    anyhow!("transport error talking to {base_url}: {err}")
}

/// Return a UTF-8-lossy preview of the first [`BODY_PREVIEW_BYTES`] bytes
/// of `body`, with bytes beyond the cap omitted.
fn preview_body(body: &[u8]) -> String {
    let cut = BODY_PREVIEW_BYTES.min(body.len());
    String::from_utf8_lossy(&body[..cut]).to_string()
}

#[cfg(test)]
#[path = "http_client_tests.rs"]
mod tests;
