# Configuration Reference -- syslog-mcp

Complete configuration reference. syslog-mcp uses compiled defaults, optional
TOML config, the shared setup env file, and process environment overrides.

## Configuration precedence

Precedence (highest to lowest):
1. Environment variables (always win)
2. `~/.syslog-mcp/.env` (or `$SYSLOG_MCP_HOME/.env`) when present
3. `config.toml` in the working directory (partial configs supported -- missing fields keep defaults)
4. Compiled defaults in `src/config.rs`

The setup env file is created and repaired by `syslog setup`. It is loaded
automatically by installed CLI commands so `syslog stats`, `syslog mcp`, and
`syslog serve mcp` see the same database path and runtime settings as the
Docker Compose container. Explicit process environment variables still win.

## config.toml

The TOML config file at the repo root is used for local development. It is **not** copied into the Docker image -- container deployments use defaults + env vars exclusively.

```toml
[syslog]
host = "0.0.0.0"
port = 1514
max_message_size = 8192

[storage]
db_path = "/data/syslog.db"
pool_size = 4
retention_days = 90
wal_mode = true
max_db_size_mb = 1024
recovery_db_size_mb = 900
min_free_disk_mb = 512
recovery_free_disk_mb = 768
cleanup_interval_secs = 60

[mcp]
host = "0.0.0.0"
port = 3100
server_name = "syslog-mcp"
allowed_hosts = ["syslog.example.com", "syslog.example.com:443"]
allowed_origins = ["https://syslog.example.com"]

[api]
enabled = false

[docker_ingest]
enabled = false
reconnect_initial_ms = 1000
reconnect_max_ms = 30000

[[docker_ingest.hosts]]
name = "edge-host-a"
base_url = "http://edge-host-a:2375"
allow_insecure_http = true
```

Bind host fields (`SYSLOG_HOST` and `SYSLOG_MCP_HOST`) must be hostnames or IP
addresses without `:` because their ports are configured separately.
`allowed_hosts` / `SYSLOG_MCP_ALLOWED_HOSTS` are RMCP Host-header allow-list
entries and may include `host:port` values such as `syslog.example.com:443`.
`allowed_origins` / `SYSLOG_MCP_ALLOWED_ORIGINS` remain full browser origin URLs
such as `https://syslog.example.com`.

## Environment variables

### Syslog listener (`SYSLOG_*`)

| Variable | Required | Default | Sensitive | Description |
| --- | --- | --- | --- | --- |
| `SYSLOG_HOST` | no | `0.0.0.0` | no | Listen host for UDP+TCP syslog (no port -- use separate setting) |
| `SYSLOG_PORT` | no | `1514` | no | Listen port shared by UDP and TCP syslog listeners |
| `SYSLOG_MAX_MESSAGE_SIZE` | no | `8192` | no | Max bytes per UDP datagram or newline-delimited TCP frame. Oversized frames are dropped. |
| `SYSLOG_MAX_TCP_CONNECTIONS` | no | `1024` | no | Maximum simultaneous TCP syslog connections |
| `SYSLOG_TCP_IDLE_TIMEOUT_SECS` | no | `300` | no | Idle timeout per TCP read before closing inactive connections |
| `SYSLOG_BATCH_SIZE` | no | `100` | no | Entries per batch flush to SQLite |
| `SYSLOG_FLUSH_INTERVAL` | no | `500` | no | Batch flush interval in milliseconds |
| `SYSLOG_WRITE_CHANNEL_CAPACITY` | no | `10000` | no | Internal parsed-message queue capacity |

TCP forwarders can keep a connection open and send multiple newline-delimited syslog frames. The size limit applies to each frame, not to the full TCP connection lifetime. An oversized newline-delimited frame is dropped and later bounded frames on the same connection can still be ingested. An oversized unterminated frame is dropped and the connection is closed because the listener cannot safely find the next frame boundary.

### Docker socket-proxy ingest (`SYSLOG_DOCKER_*`)

This optional mode pulls stdout/stderr logs from remote Docker hosts through `docker-socket-proxy` instead of changing Docker's daemon-level logging driver. Containers keep their existing local logging behavior, and remote host/container startup does not depend on syslog-mcp being online.

Set `SYSLOG_DOCKER_HOSTS` to a comma-separated list of hostnames. Each hostname becomes `http://<host>:2375` with insecure HTTP allowed — use only on trusted private networks.

```env
SYSLOG_DOCKER_HOSTS=squirts,tootie,dookie
```

`SYSLOG_DOCKER_HOSTS_FILE` (path to a legacy `[[hosts]]` TOML file) is still accepted as a fallback when `SYSLOG_DOCKER_HOSTS` is not set.

| Variable | Required | Default | Sensitive | Description |
| --- | --- | --- | --- | --- |
| `SYSLOG_DOCKER_INGEST_ENABLED` | no | `false` | no | Enable pull-based Docker log ingestion |
| `SYSLOG_DOCKER_HOSTS` | yes, if Docker ingest is enabled | (none) | no | Comma-separated hostnames — each becomes `http://<host>:2375` |
| `SYSLOG_DOCKER_RECONNECT_INITIAL_MS` | no | `1000` | no | Initial reconnect delay after host stream failure |
| `SYSLOG_DOCKER_RECONNECT_MAX_MS` | no | `30000` | no | Maximum reconnect delay after repeated failures |

Minimum recommended docker-socket-proxy permissions on each remote host:

```env
CONTAINERS=1
EVENTS=1
PING=1
VERSION=1
POST=0
```

`CONTAINERS=1` exposes the broader read-only Docker container API to every client that can reach docker-socket-proxy. Bind the proxy on a trusted private network, firewall it so only syslog-mcp can connect, or put it behind authenticated TLS. Hosts using plain `http://` must set `allow_insecure_http = true` in the hosts file; otherwise config validation rejects them.

Docker ingest is not included in the default smoke test because it requires a live docker-socket-proxy-compatible endpoint. For integration coverage, run a disposable docker-socket-proxy or mocked Docker HTTP fixture, set `SYSLOG_DOCKER_INGEST_ENABLED=true`, emit a unique container stdout/stderr line, and verify it with `search` or `tail`. Container stream rows identify their source as `docker://<host>/<container>/<stream>`. Container lifecycle events such as `create`, `start`, `restart`, `die`, `stop`, `destroy`, `rename`, and `oom` identify their source as `docker-event://<host>/<container>/<action>` and use `facility=docker`.

### MCP server (`SYSLOG_MCP_*`)

| Variable | Required | Default | Sensitive | Description |
| --- | --- | --- | --- | --- |
| `SYSLOG_MCP_HOST` | no | `0.0.0.0` | no | HTTP listen host for MCP endpoint |
| `SYSLOG_MCP_PORT` | no | `3100` | no | HTTP listen port for MCP endpoint |
| `SYSLOG_MCP_TOKEN` | no | (none) | **yes** | Bearer token for `/mcp` auth. Generate: `openssl rand -hex 32`. When unset, auth is disabled. |
| `SYSLOG_MCP_ALLOWED_HOSTS` | no | (none) | no | Extra comma-separated Host header values for RMCP Host validation |
| `SYSLOG_MCP_ALLOWED_ORIGINS` | no | (none) | no | Extra comma-separated browser origins for RMCP Origin validation |

### Non-MCP API (`SYSLOG_API_*`)

The plain JSON API is disabled by default. When enabled, it is mounted under `/api/*` on the same HTTP listener and requires its own bearer token.

| Variable | Required | Default | Sensitive | Description |
| --- | --- | --- | --- | --- |
| `SYSLOG_API_ENABLED` | no | `false` | no | Enable the non-MCP JSON API |
| `SYSLOG_API_TOKEN` | yes, when enabled | (none) | **yes** | Bearer token for `/api/*` routes |

### Headless Gemini assessment (`SYSLOG_HEADLESS_*`, `SYSLOG_LLM_*`)

`syslog ai assess` is local-only and starts the Gemini CLI in an isolated
temporary HOME. It copies Gemini auth files from the configured source HOME,
installs the bundled `syslog-frustration-assessment` skill into that isolated
HOME, disables MCP servers/hooks/context-file loading, and parses Gemini's
`stream-json` output so only assistant text is emitted.

| Variable | Required | Default | Sensitive | Description |
| --- | --- | --- | --- | --- |
| `SYSLOG_HEADLESS_GEMINI_CMD` | no | `gemini` | no | Gemini CLI executable path or command name |
| `SYSLOG_HEADLESS_GEMINI_MODEL` | no | `gemini-3.1-flash-lite-preview` | no | Default model for `syslog ai assess`; `--model` on the CLI overrides this |
| `SYSLOG_HEADLESS_GEMINI_HOME` | no | `$HOME` | maybe | Source home containing `.gemini` auth files to copy into the isolated runtime HOME |
| `SYSLOG_LLM_COMPLETION_TIMEOUT_SECS` | no | `120` | no | Hard timeout for the Gemini assessment subprocess |

### Storage (`SYSLOG_MCP_*`)

| Variable | Required | Default | Sensitive | Description |
| --- | --- | --- | --- | --- |
| `SYSLOG_MCP_DB_PATH` | no | `/data/syslog.db` | no | Path to SQLite database file |
| `SYSLOG_MCP_POOL_SIZE` | no | `4` | no | SQLite connection pool size (must be > 0) |
| `SYSLOG_MCP_RETENTION_DAYS` | no | `90` | no | Days to retain logs before automatic hourly purge (0 = keep forever) |

### Storage budget (`SYSLOG_MCP_*`)

| Variable | Required | Default | Sensitive | Description |
| --- | --- | --- | --- | --- |
| `SYSLOG_MCP_MAX_DB_SIZE_MB` | no | `1024` | no | Soft limit for logical DB size in MB (0 = disable) |
| `SYSLOG_MCP_RECOVERY_DB_SIZE_MB` | no | `900` | no | Cleanup target after DB-size breach (must be < max) |
| `SYSLOG_MCP_MIN_FREE_DISK_MB` | no | `512` | no | Minimum free disk space in MB (0 = disable) |
| `SYSLOG_MCP_RECOVERY_FREE_DISK_MB` | no | `768` | no | Cleanup target after free-disk breach (must be > min) |
| `SYSLOG_MCP_CLEANUP_INTERVAL_SECS` | no | `60` | no | Storage budget enforcement interval in seconds (minimum 5) |
| `SYSLOG_MCP_CLEANUP_CHUNK_SIZE` | no | `2000` | no | Rows deleted per chunk during enforcement (1 to 1,000,000) |

### Logging

| Variable | Required | Default | Sensitive | Description |
| --- | --- | --- | --- | --- |
| `RUST_LOG` | no | `info` | no | Rust tracing filter directive. Examples: `debug`, `syslog_mcp=debug,tower_http=info`, `trace` |

### Docker / container

| Variable | Required | Default | Sensitive | Description |
| --- | --- | --- | --- | --- |
| `SYSLOG_UID` | no | `1000` | no | Container user ID |
| `SYSLOG_GID` | no | `1000` | no | Container group ID |
| `SYSLOG_PORT` | no | `1514` | no | Host-side syslog port mapping |
| `SYSLOG_MCP_PORT` | no | `3100` | no | Host-side MCP port mapping |
| `SYSLOG_MCP_DATA_VOLUME` | no | `syslog-mcp-data` | no | Named Docker volume for `/data` |
| `SYSLOG_MCP_CONFIG_VOLUME` | no | `./config` | no | Read-only config mount for optional files such as `docker-hosts.toml` |
| `DOCKER_NETWORK` | no | `syslog-mcp` | no | External Docker network name |

## Storage budget behavior

The storage budget is a two-threshold system with hysteresis to prevent oscillation:

1. **Trigger threshold**: When logical DB size exceeds `max_db_size_mb` or free disk drops below `min_free_disk_mb`, enforcement begins.
2. **Recovery target**: Oldest logs are deleted in chunks until logical DB size drops below `recovery_db_size_mb` and free disk rises above `recovery_free_disk_mb`.
3. **Write blocking**: If cleanup cannot recover enough space (e.g., no more logs to delete), the batch writer blocks new writes until storage becomes healthy.
4. **Enforcement interval**: Checked every `cleanup_interval_secs` seconds (default 60).

Set both `max_db_size_mb` and `min_free_disk_mb` to 0 to disable all storage enforcement.

## SQLite migration upgrades

Startup creates missing schema objects automatically. Small migrations are expected to complete quickly, but heavyweight migrations on a populated database can hold SQLite's write lock before syslog listeners and `/health` are ready. The server logs an operator-visible `Migration N: starting ...` message before such work and a completion message with elapsed time.

For populated databases, treat heavy migrations as a planned upgrade step:

1. Stop or quiet high-volume senders if packet loss is unacceptable.
2. Take a WAL-safe backup with `scripts/backup.sh` or `sqlite3 /data/syslog.db ".backup /data/syslog-pre-upgrade.db"`.
3. Start the upgraded container or binary and watch `docker compose logs -f syslog-mcp` or the relevant service log for migration start/completion lines.
4. Wait for `curl -sf http://localhost:3100/health` to succeed.
5. Run `syslog stats --json` or `mcporter call ... action=stats` and confirm `total_logs`, storage metrics, and `write_blocked` match expectations.

If a migration must be abandoned, stop the new process before changing files, restore the WAL-safe backup, and restart the previous image or binary. See [runbooks/deploy.md](runbooks/deploy.md) for the full deploy checklist.

## Validation rules

- `SYSLOG_MCP_POOL_SIZE` must be > 0
- `recovery_db_size_mb` must be > 0 and < `max_db_size_mb` when DB size guard is enabled
- `recovery_free_disk_mb` must be > 0 and > `min_free_disk_mb` when free-disk guard is enabled
- `cleanup_interval_secs` must be >= 5
- `cleanup_chunk_size` must be between 1 and 1,000,000
- `SYSLOG_API_TOKEN` is required when `SYSLOG_API_ENABLED=true`
- Bind host fields (`SYSLOG_HOST`, `SYSLOG_MCP_HOST`) must not contain a colon (port is a separate setting)
- `SYSLOG_MCP_ALLOWED_HOSTS` values may include `host:port` to match reverse-proxy Host headers
- `SYSLOG_DOCKER_HOSTS` must contain at least one hostname when Docker ingest is enabled
- Docker ingest host names must be unique

## Plugin deployment

`syslog serve mcp` runs as a daemon (syslog listener + HTTP MCP server), so the plugin connects via HTTP -- not stdio.

When installed as a Claude Code plugin, users are prompted for:

| Field | Sensitive | Description |
| --- | --- | --- |
| `server_url` | no | Base server URL (e.g. `https://syslog.example.com`) |
| `api_token` | yes | Bearer token used by the plugin MCP client; enforced by the server unless `no_auth=true` |
| `no_auth` | no | Explicit no-auth mode; non-loopback server binds also require `SYSLOG_MCP_TRUSTED_GATEWAY_NO_AUTH=true` |
| `is_server` | no | Whether this host owns the Docker Compose deployment |

These values are interpolated into `plugins/syslog/.mcp.json` via `${user_config.*}` syntax. See [plugin/CONFIG.md](plugin/CONFIG.md) for details.

## .env.example conventions

- Group variables by section with comment headers
- Required variables first within each group
- No actual secrets -- use descriptive placeholders
- See `.env.example` at the repo root for the full template
