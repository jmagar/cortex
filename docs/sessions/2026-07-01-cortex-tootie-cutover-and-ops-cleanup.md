# 2026-07-01 Cortex Tootie Cutover and Ops Cleanup

## Metadata

- Date: 2026-07-01 15:38:24 EST
- Repository: `git@github.com:jmagar/cortex.git`
- Working directory: `/home/jmagar/workspace/cortex`
- Branch: `main`
- HEAD at capture: `73318a2`
- Worktree: `/home/jmagar/workspace/cortex 73318a2 [main]`
- Transcript discovered by local Claude metadata scan: `/home/jmagar/.claude/projects/-home-jmagar-workspace-cortex/8e2881c3-9d86-4c87-b604-0d26f03652ea.jsonl`
- Related beads: `syslog-mcp-4n4a6`, `syslog-mcp-8by8d`

The discovered Claude transcript path appears to be an older/stale Claude session artifact rather than the current Codex app transcript. This note is therefore reconstructed from the live operations performed and verification output captured during the session.

## Summary

Production Cortex was moved from dookie to tootie as a fresh deployment using the GHCR image and an appdata bind mount. The old dookie production container path was retired for production, SWAG was repointed to tootie, agents were repointed to the new production target, and a fresh fleet health pass proved current ingest from the expected hosts.

After the cutover, live Cortex checks surfaced three recurring noise sources: missing Labby service wrapper on squirts, a stale dookie `cortex-agent-command-ingest` timer using removed CLI grammar, and a Tracearr fixture server on SHART polling `example.invalid`. All three were investigated from evidence, fixed at the runtime/config layer, and checked again through Cortex search and fleet state.

## What Changed

Production deployment:

- Created tootie production appdata layout under `/mnt/cache/appdata/cortex`.
- Created the tootie Compose deployment under `/mnt/cache/appdata/cortex/compose`.
- Started `ghcr.io/jmagar/cortex:3.1.3` on tootie with ports `1514/tcp`, `1514/udp`, and `3100/tcp`.
- Used a fresh DB at `/mnt/cache/appdata/cortex/data/cortex.db`; no dookie DB rsync was performed by request.
- Disabled central Docker remote ingest on the server with `CORTEX_INVENTORY_REMOTE_DOCKER_EVENTS=false`.
- Stopped the old dookie production Cortex container.
- Disabled the old dookie `cortex-auto-deploy.timer`.

Routing and agents:

- Updated SWAG on squirts so `cortex.tootie.tv` upstreams to tootie LAN address `10.1.0.2` instead of dookie `100.88.16.79`.
- Ran `nginx -t` and reloaded SWAG after the upstream change.
- Repointed Linux/WSL agents to production Cortex:
  - dookie, squirts, steamy-wsl, and vivobook-wsl now use `CORTEX_HEARTBEAT_TARGET=https://cortex.tootie.tv` and `CORTEX_SYSLOG_TARGET=10.1.0.2:1514`.
  - tootie agent sends to `127.0.0.1:1514`.
  - shart agent sends to `10.1.0.2:1514`.
- Updated tootie and shart agent containers to `ghcr.io/jmagar/cortex:3.1.3`.
- Recreated the shart agent container with `--no-healthcheck` because the image healthcheck curls a local server on `localhost:3100`, which is valid for server containers but misleading for agent-only containers.

Operational cleanup:

- Restored `lab-serve.service` on squirts by installing `~/.local/bin/labby` and creating `~/.local/bin/lab-serve-wrapper.sh`.
- Restored the Labby controller on dookie with `~/.local/bin/lab-serve-wrapper.sh` and a user `lab-serve.service`.
- Disabled the stale dookie user `cortex-agent-command-ingest.timer` and reset the failed service state.
- Removed the Tracearr fixture server row from SHART's `tracearr-db`:
  - server name: `Rustarr Fixture Server`
  - URL: `http://example.invalid`
  - related `server_users` row removed first
  - Tracearr restarted afterward

Beads:

- Added a live ops note to `syslog-mcp-4n4a6` explaining that dookie's stale command-ingest timer was disabled after the production move because current `cortex ingest agent-command` is local-DB only and rejects `--http/--server/--token`.
- Created `syslog-mcp-8by8d` to update stale docs that still describe dookie as active production or point forwarders at `100.88.16.79`.

## Verification Evidence

- `curl -k -fsS https://cortex.tootie.tv/health` returned `{"status":"ok"}`.
- MCP `status` succeeded with `db_ok=true`, TCP and UDP listeners alive, queue utilization `0.00`, and no write failures.
- Fleet state reached `6/6` hosts with heartbeats ok during the cutover verification.
- Later fleet state after cleanup showed 6 hosts total, 5 ok, 1 pressure, 0 late, and 0 partial; the remaining pressure was `dookie` swap pressure, not an ingest outage.
- Recent Cortex logs showed expected host coverage from dookie, tootie, squirts, SHART, STEAMY, and vivobook.
- Docker log forwarding was enabled on dookie, squirts, tootie, and shart.
- Journald forwarding was active on Linux/WSL agents.
- WSL Docker forwarding remained disabled as expected.
- After fixing squirts Labby service, `curl http://127.0.0.1:8765/health` on squirts returned `{"ok":true}`.
- After fixing dookie Labby controller, `curl http://127.0.0.1:8765/health` on dookie returned controller health with `mode":"master"`.
- After fixing Tracearr, the fixture server count for `example.invalid` in SHART's Tracearr database was `0`.
- Fresh Cortex searches after the fixes showed no new hits in the checked window for:
  - `lab-serve-wrapper.sh`
  - `cortex-agent-command-ingest.service`
  - `example.invalid`

## Commands Used

Representative commands from the session:

```bash
ssh tootie
ssh squirts
ssh shart
ssh dookie

curl -k -fsS https://cortex.tootie.tv/health

cortex tail -n 25 --json
cortex analysis errors --since 15m --limit 20 --json
cortex apps --since 15m --limit 12 --json
cortex state fleet --json

systemctl --user status lab-serve.service
systemctl --user cat lab-serve.service
systemctl --user restart lab-serve.service
systemctl --user enable --now lab-serve.service

systemctl --user disable --now cortex-agent-command-ingest.timer
systemctl --user reset-failed cortex-agent-command-ingest.service

docker compose up -d
docker compose logs --tail 100
docker exec tracearr-db psql -U tracearr -d tracearr
```

Repo-status capture after the ops work showed:

- Current checkout: `main`
- Local branch state: synced with `origin/main`
- Worktrees: single checkout at `/home/jmagar/workspace/cortex`
- Local branches: `main`
- Remote refs: `origin/main`, `origin/marketplace-no-mcp`
- Open PRs: none
- Recent main CI: green

## Files and Runtime Artifacts Touched

Repo artifact created by this save:

- `/home/jmagar/workspace/cortex/docs/sessions/2026-07-01-cortex-tootie-cutover-and-ops-cleanup.md`

Remote/runtime artifacts changed during the session:

- tootie: `/mnt/cache/appdata/cortex/compose/`
- tootie: `/mnt/cache/appdata/cortex/data/`
- tootie: Cortex server and agent containers
- squirts: `/mnt/appdata/swag/nginx/proxy-confs/syslog.subdomain.conf`
- squirts: `~/.local/bin/labby`
- squirts: `~/.local/bin/lab-serve-wrapper.sh`
- dookie: `~/.local/bin/lab-serve-wrapper.sh`
- dookie: user `lab-serve.service`
- dookie: user `cortex-agent-command-ingest.timer`
- shart: Tracearr Postgres `servers` and `server_users` rows for the fixture server
- host agent environment/config on dookie, squirts, tootie, shart, steamy-wsl, and vivobook-wsl

## Open Follow-Ups

- `syslog-mcp-8by8d`: update stale docs after the production move. Evidence found during save:
  - `docs/runbooks/deploy.md` still says the active dookie deployment is the local source-built Compose stack.
  - `docs/contracts/forwarder-dropins.md` still contains dookie and `100.88.16.79` forwarder examples.
- `syslog-mcp-4n4a6`: decide whether command spool ingest should support forwarding to production Cortex or whether setup should retire stale local timers when production host changes.
- Keep an eye on dookie's swap pressure separately; it was the only remaining fleet pressure flag seen after cleanup and was not caused by the fixed log-ingest issues.

## Rollback Notes

- To roll production back to dookie, restore the SWAG upstream to dookie, restart the old dookie Compose/server path, and re-enable the appropriate dookie deployment automation.
- To undo the dookie command-ingest timer decision, re-enable the user timer only after the command path is updated for the current CLI semantics and production target model.
- To restore the Tracearr fixture server, recreate the removed server and related user association in Tracearr's database or through the app if that fixture is intentionally needed.
- To undo the Labby service repairs, disable the user services and remove the wrapper scripts, but the current state is healthier than the missing-wrapper state that produced the Cortex errors.
