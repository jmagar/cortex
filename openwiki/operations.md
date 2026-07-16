# Operations

Deployment, configuration, notifications, and operational guidance for running cortex in production.

## Deployment

### Docker Compose (Recommended)
cortex is designed to run as a Docker Compose service:

```bash
# Start the stack
docker compose up -d

# Check health
curl http://localhost:3100/health | jq

# View logs
docker compose logs -f cortex

# Stop
docker compose down
```

**Key files**:
- `docker-compose.yml`: Development configuration
- `docker-compose.prod.yml`: Production configuration
- `config/Dockerfile`: Multi-stage Rust build

### Ownership Resolution
cortex provides tools to resolve the canonical Compose owner:

- `cortex compose status --json`: Inspect canonical container/project
- `cortex compose doctor`: Strict health diagnostics
- `cortex compose pull`: Pull image for resolved service
- `cortex compose up`: Run `docker compose up -d` for resolved service
- `cortex compose restart`: Restart resolved service
- `cortex compose logs --tail 20`: Bounded compose logs

**Key files**:
- `src/compose/`: Compose lifecycle management
- `src/doctor.rs`: Health diagnostics

### Host-local Agent
Deploy the cortex agent on each Docker host for log streaming:

- **Purpose**: Stream container logs from local Docker socket
- **Deployment**: Push via SSH or systemd unit
- **Config**: Agent deployment config in `~/.cortex/agent/`

**Key files**:
- `src/agent/`: Host-local agent implementation
- `src/deploy.rs`: Agent deployment (push rsyslog configs via SSH)

## Configuration

### Layered Config
cortex loads configuration in layers (later layers override earlier ones):

1. **Defaults**: Hardcoded in `src/config.rs`
2. **config.toml**: Repository-level config file
3. `~/.cortex/.env`: User-level env file (created by `cortex setup repair`)
4. **Process env vars**: `CORTEX_*` environment variables

**Key files**:
- `config/config.toml`: Repository config (ports, paths, defaults)
- `.env.example`: Sample environment variables
- `src/config.rs`: Config loading and validation

### Key Configuration Sections

#### Listener
```toml
[listener]
host = "0.0.0.0"
port = 1514
```

#### MCP
```toml
[mcp]
host = "0.0.0.0"
port = 3100
```

#### Storage
```toml
[storage]
data_dir = "/data"
retention_days = 0
max_db_size_mb = 0
min_free_disk_mb = 0
```

#### OAuth
```toml
[oauth]
enabled = true
client_id = "..."
client_secret = "..."
```

**Complete reference**: **[docs/CONFIG.md](../docs/CONFIG.md)**

### Environment Variables

Core variables:
- `CORTEX_RECEIVER_HOST`: Syslog bind address (default: 0.0.0.0)
- `CORTEX_RECEIVER_PORT`: Syslog port (default: 1514)
- `CORTEX_MCP_HOST`: MCP bind address (default: 0.0.0.0)
- `CORTEX_MCP_PORT`: MCP port (default: 3100)
- `CORTEX_TOKEN`: Static bearer token for MCP
- `CORTEX_API_TOKEN`: Bearer token for REST API
- `CORTEX_USE_HTTP`: Force CLI to use HTTP mode (default: true since v0.26)
- `CORTEX_RETENTION_DAYS`: Global retention (0 disables)
- `CORTEX_MAX_DB_SIZE_MB`: Max DB size before cleanup
- `CORTEX_MIN_FREE_DISK_MB`: Min free disk before write-block

**Complete reference**: **[docs/CONFIG.md](../docs/CONFIG.md)**

## Notifications

### Overview
cortex uses Apprise for notifications with rule-based evaluation and daily digests.

### Configuration
```toml
[notifications]
enabled = true
apprise_url = "mailto://admin@example.com"

[notifications.dispatcher]
interval_secs = 30

[notifications.evaluators]
evaluator_interval_secs = 300
digest_cron_local = "0 8 * * *"

[[notifications.evaluators.rules]]
type = "oom_kill"
enabled = true

[[notifications.evaluators.rules]]
type = "ingest_silence"
enabled = true
threshold_secs = 300
```

### Notification Types
- `oom_kill`: Container OOM kills detected in logs
- `ingest_silence`: No logs received in threshold window
- `error_signature`: New repeating error signatures
- `storage_budget`: DB size or disk space warnings

### Commands
- `cortex notifications recent`: List recent notification firings
- `cortex notifications test`: Send test notification (admin scope)

**Key files**:
- `src/notifications/`: Apprise dispatcher and rule evaluators
- `src/db/notifications.rs`: Notification history queries

## Health & Diagnostics

### Health Endpoints
- `GET /health`: Basic health (DB liveness, listener state)
- `GET /health/full`: Full health snapshot (maintenance state, queue depth, OTLP counters)

### Health Checks
- **DB liveness**: `PRAGMA quick_check` passes
- **Syslog listeners**: UDP and TCP listeners alive (not down)
- **Ingest rate**: Recent throughput (logs/sec)
- **Write-block**: Storage budget state (blocked/unblocked)
- **Queue depth**: Ingest channel backlog

### Diagnostics
- `cortex doctor`: Interactive diagnostics (checks config, DB, listeners, storage)
- `cortex compose doctor`: Compose-specific diagnostics (owner resolution, service health)
- `cortex status`: Lightweight runtime status

**Key files**:
- `src/doctor.rs`: Diagnostics implementation
- `src/runtime/observability.rs`: Runtime metrics

## Maintenance

### Automated Tasks
See **[Architecture](architecture.md)** for the full task list.

### Manual Operations

#### Backup
```bash
cortex db backup
```
Creates WAL-safe backup at `backup/` with timestamp.

#### Integrity Check
```bash
cortex db integrity
```
Runs `PRAGMA integrity_check` on the database.

#### Maintenance Status
```bash
cortex db status
```
Shows retention policy, storage budget, last maintenance runs.

#### Retention Override
```bash
cortex db purge --days 30
```
Manually purge logs older than N days (overrides `CORTEX_RETENTION_DAYS`).

**Key files**:
- `src/db/maintenance.rs`: Retention and storage budget enforcement
- `src/cli/dispatch_db.rs`: CLI DB commands

## Troubleshooting

### Ingest Issues

#### No logs arriving
1. Check listener health: `cortex status` or `GET /health/full`
2. Check firewall: Ensure UDP/TCP 1514 is open
3. Check syslog sender: Verify rsyslog/syslog-ng config
4. Check rate: `cortex stats ingestrate` for throughput

#### High queue depth
- Cause: Ingest faster than write throughput
- Fix: Check storage budget (write-block), disk I/O

### Storage Issues

#### Write-blocked
- Cause: Free disk below `CORTEX_MIN_FREE_DISK_MB`
- Fix: Free disk space or increase threshold
- Check: `cortex stats` for `write_blocked` flag

#### DB size growing
- Cause: `CORTEX_MAX_DB_SIZE_MB` not set or too high
- Fix: Set appropriate limit and let retention purge old logs
- Check: `cortex db status` for DB size

### Performance Issues

#### Slow queries
- Run `PRAGMA optimize`: `cortex db status` shows last run
- Check timeline rollup freshness
- Verify FTS5 index is intact

#### High memory
- Check SQLite cache size (default is sensible)
- Reduce batch size if ingest is bursty

### Listener Failures

#### Syslog listener down
- Check `syslog_udp_listener_state` or `syslog_tcp_listener_state` in `/health/full`
- Listeners auto-restart with backoff
- Check port conflicts (another service on 1514)

## Security

### Credentials
- Never commit `CORTEX_TOKEN` or `CORTEX_API_TOKEN`
- Use strong random tokens (generate with `just gen-token`)
- Prefer OAuth/JWT for production (see **[docs/OAUTH.md](../docs/OAUTH.md)**)

### Docker Socket
- Host-local agent requires Docker socket access
- Use `docker-socket-proxy` for central pull mode
- Follow principle of least privilege

### Network Exposure
- Default: MCP binds to `0.0.0.0` (all interfaces)
- Restrict with firewall rules or `CORTEX_MCP_HOST`
- OTLP `/v1/logs` exposure blocked at startup unless `CORTEX_TOKEN` is set

**Complete guidance**: **[docs/SECURITY.md](../docs/SECURITY.md)**

## Monitoring

### Key Metrics
- Ingest rate (logs/sec)
- Queue depth (messages in batch channel)
- DB size and growth rate
- Free disk space
- Listener state (alive/down)
- Write-block state

### Log Aggregation
- cortex logs to stdout/stderr (structured JSON)
- Configure log level via `RUST_LOG` env var
- Ship logs to external aggregator (e.g., Loki, ELK)

### Alerting
- Configure Apprise notifications (see above)
- Set up alerting on health endpoints (`/health` returns 503 on failure)
- Monitor storage budget and write-block state

## References

- **[docs/SETUP.md](../docs/SETUP.md)** – Step-by-step setup guide
- **[docs/CONFIG.md](../docs/CONFIG.md)** – Complete configuration reference
- **[docs/OAUTH.md](../docs/OAUTH.md)** – OAuth/JWT configuration
- **[docs/runbooks/deploy.md](../docs/runbooks/deploy.md)** – Deployment runbook
- **[docs/SECURITY.md](../docs/SECURITY.md)** – Security guidance
