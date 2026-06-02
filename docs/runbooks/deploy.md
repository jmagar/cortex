# Deploy Runbook — cortex

## Rolling Update

```bash
# 1. Pull latest code
git pull origin main

# 2. Diagnose the current owner before mutation
cortex compose doctor

# 3. Run smoke test against current running instance (optional)
bash scripts/smoke-test.sh

# 4. Pull/build as needed, then start the resolved Compose service
cortex compose pull
cortex compose up

# 5. Wait for health check to pass (30s interval, 3 retries)
cortex compose status   # should show healthy Compose ownership
# or:
curl -sf http://localhost:3100/health

# 6. Verify logs are flowing
cortex compose logs --tail 20
```

## Rollback

```bash
# Option 1: Revert to previous image (if tagged)
cortex compose down --yes
git checkout <previous-tag>
cortex compose pull
cortex compose up

# Option 2: Revert to previous commit
git log --oneline -5   # find the good commit
git revert HEAD         # or git reset --hard <sha>
cortex compose pull && cortex compose up
```

## Health Check

The container includes a built-in healthcheck (`wget -q --spider http://localhost:3100/health`).
Docker will mark the container unhealthy after 3 consecutive failures (30s interval, 5s timeout).

```bash
# Check container health status
docker inspect --format='{{.State.Health.Status}}' cortex
```

## Pre-deploy Checklist

- [ ] `cargo test` passes locally
- [ ] `cargo clippy` has no warnings
- [ ] No uncommitted changes (`git status` clean)
- [ ] Database backup taken (see backup section below)
- [ ] For populated databases, review [Heavy SQLite Migration Upgrade](#heavy-sqlite-migration-upgrade) before restarting

## Database Backup Before Deploy

```bash
# WAL-safe online backup (no downtime)
docker compose exec cortex sqlite3 /data/cortex.db ".backup /data/syslog-pre-deploy.db"
```

The repo also includes `scripts/backup.sh`, which performs a WAL-safe checkpoint and SQLite backup from the host when the database path is reachable.

## Heavy SQLite Migration Upgrade

Most schema migrations run automatically at startup and are safe for normal rolling updates. Some migrations are intentionally heavier, such as creating an index on a populated `logs` table. Those can hold SQLite's write lock for several minutes before `/health` responds or syslog listeners start, so treat them as a planned ingest maintenance window on large databases.

Use this path when release notes, startup logs, or `docs/CONFIG.md` indicate a heavy migration:

```bash
# 1. Confirm current database size and baseline counts.
docker compose exec cortex sqlite3 /data/cortex.db \
  "SELECT COUNT(*) FROM logs; PRAGMA page_count; PRAGMA page_size;"

# 2. Take a WAL-safe backup.
docker compose exec cortex sqlite3 /data/cortex.db ".backup /data/syslog-pre-heavy-migration.db"

# 3. Build or pull the new version, then start it.
docker compose build
docker compose up -d

# 4. Watch for migration start/completion lines.
docker compose logs -f cortex

# 5. Verify health and storage state after completion.
curl -sf http://localhost:3100/health
mcporter call --config config/mcporter.json syslog.cortex action=stats
```

Expected operator signals:

- Startup logs include `Migration N: starting ...` before a heavyweight operation.
- Completion logs include the migration name and elapsed time.
- `/health` may fail until the migration commits.
- UDP senders can lose packets while the listener is unavailable; TCP senders may reconnect or buffer depending on their own config.

Rollback while a migration is running is a restore operation, not a partial schema edit. Stop the new process, restore the WAL-safe backup to `/data/cortex.db`, then start the previous image or binary.

## Docker Ingest Integration Check

The default `scripts/smoke-test.sh` covers live UDP and TCP ingest plus MCP actions. Docker ingest is heavier because it requires a docker-socket-proxy-compatible endpoint and a container log stream, so run it explicitly during Docker ingest changes:

1. Start a disposable docker-socket-proxy or mocked Docker HTTP fixture with `CONTAINERS=1`, `EVENTS=1`, `PING=1`, `VERSION=1`, and `POST=0`.
2. Start cortex with `CORTEX_DOCKER_INGEST_ENABLED=true` and `CORTEX_DOCKER_HOSTS=<fixture-host>`.
3. Run a short-lived container that writes a unique marker to stdout and stderr.
4. Verify `search` or `tail` returns the marker and that stream rows use `source_ip=docker://<host>/<container>/<stream>`.
5. Restart or recreate the disposable container and verify lifecycle rows use `source_ip=docker-event://<host>/<container>/<action>` with `facility=docker`.
5. Stop the fixture and confirm cortex logs reconnect/backoff without crashing.
