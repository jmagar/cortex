# Fleet Deployment — syslog-mcp expansion

These artifacts deploy the operational side of epic
`syslog-mcp-6uoy` (full-fleet log ingest + OTLP). Phase 1 (OTLP receiver)
and Phase 2 (enrichment + retention) are pure Rust changes already in the
syslog-mcp binary; the files in this directory are the manual deployment
steps for Phases 3–5.

**Deploy mechanism: manual SSH per host.** No Ansible, no automation — match
the rest of the homelab's posture.

---

## Phase 3 — host-wide journald + AI transcripts

Hosts: **dookie, squirts, steamy-wsl, vivobook-wsl**.

### Prerequisites

* WSL hosts (steamy-wsl, vivobook-wsl) need `[boot] systemd=true` in
  `/etc/wsl.conf`, then `wsl --shutdown` from PowerShell to restart. Verify
  with `systemctl status` after restart — must show "running."
* `/var/spool/rsyslog/` must exist on every host. Create with
  `sudo mkdir -p /var/spool/rsyslog && sudo chown syslog:syslog /var/spool/rsyslog`
  if absent.

### Deploy (per host)

```bash
# imjournal — all four hosts
scp deploy/rsyslog/10-imjournal.conf <host>:/tmp/
ssh <host> 'sudo mv /tmp/10-imjournal.conf /etc/rsyslog.d/ \
  && sudo rsyslogd -N1 \
  && sudo systemctl restart rsyslog'

# AI transcripts — dookie, steamy-wsl, vivobook-wsl ONLY (skip squirts)
scp deploy/rsyslog/40-ai-transcripts.conf <host>:/tmp/
ssh <host> 'sudo mv /tmp/40-ai-transcripts.conf /etc/rsyslog.d/ \
  && sudo rsyslogd -N1 \
  && sudo systemctl restart rsyslog'
```

WSL alternative to `systemctl restart`: `sudo service rsyslog restart`.

### Verify

```bash
# Generate a test journald entry, confirm it arrives
ssh <host> 'logger -t deploy-test "hello from journald"'
mcporter call --config config/mcporter.json syslog-mcp.search query=deploy-test limit=5

# After a claude session runs, confirm transcripts arrive
mcporter call --config config/mcporter.json syslog-mcp.search 'tag:claude-transcript' limit=5
```

---

## Phase 4 — squirts specialty sources

Three drop-ins, deploy in the **specified order** (authelia → swag → adguard).
The order matters: AdGuard is the highest-volume source — deploy it last so
the other sources are already stable.

### Resolve `<PATH-TO-...>` placeholders first

```bash
ssh squirts 'find /mnt /opt /srv -name authelia.log     2>/dev/null | head -3'
ssh squirts 'find /mnt /opt /srv -path "*/nginx/access.log" 2>/dev/null | head -3'
ssh squirts 'find /mnt /opt /srv -path "*/nginx/error.log"  2>/dev/null | head -3'
ssh squirts 'find /mnt /opt /srv -path "*/fail2ban/fail2ban.log" 2>/dev/null | head -3'
ssh squirts 'find /mnt /opt /srv -name querylog.json    2>/dev/null | head -3'
```

Edit each `.conf` to substitute the real paths before scp'ing.

### Deploy

```bash
# 1. authelia
scp deploy/rsyslog/35-authelia.conf squirts:/tmp/
ssh squirts 'sudo mv /tmp/35-authelia.conf /etc/rsyslog.d/ \
  && sudo rsyslogd -N1 \
  && sudo systemctl restart rsyslog'

# 2. SWAG (nginx + fail2ban)
scp deploy/rsyslog/30-swag.conf squirts:/tmp/
ssh squirts 'sudo mv /tmp/30-swag.conf /etc/rsyslog.d/ \
  && sudo rsyslogd -N1 \
  && sudo systemctl restart rsyslog'

# 3. AdGuard — LAST
scp deploy/rsyslog/36-adguard.conf squirts:/tmp/
ssh squirts 'sudo mv /tmp/36-adguard.conf /etc/rsyslog.d/ \
  && sudo rsyslogd -N1 \
  && sudo systemctl restart rsyslog'
```

### Vaultwarden gap check

Before adding a vaultwarden imfile drop-in, check whether its events are
already arriving via container stdout ingestion:

```bash
mcporter call --config config/mcporter.json syslog-mcp.search query=vaultwarden limit=10
```

If you see auth events, no further action. If empty, add a drop-in similar
to `35-authelia.conf` pointed at vaultwarden's log file.

### Optional source-IP gating

Set these in syslog-mcp's environment (`/.env` or compose) to prevent other
tailnet hosts from sending crafted messages with `tag=authelia` or
`tag=adguard-query` to spoof severity/classification:

```bash
SYSLOG_MCP_AUTHELIA_SOURCE_IP=100.74.16.82   # squirts tailnet IP
SYSLOG_MCP_ADGUARD_SOURCE_IP=100.74.16.82
```

---

## Phase 5 — OTel client config (claude code + codex)

Hosts: **dookie, steamy-wsl, vivobook-wsl**.

### Prerequisite

Phase 1 must be deployed and healthy first:

```bash
curl -s http://dookie:3100/health | jq
# {"status":"ok","otlp_logs_received":0,"otlp_decode_errors":0}
```

### Claude Code

`~/.claude/settings.json` — **merge** the `env` block from
`deploy/otel/claude-code-settings.example.json` into the existing file
(do not overwrite). On a fresh host, copy the example file directly.

### Codex

`~/.codex/config.toml` — append the `[otel]` block from
`deploy/otel/codex-config.example.toml`. Create the file if absent.

### Verify

After each config change, start a new claude/codex session, then:

```bash
# Should increment after each session
curl -s http://dookie:3100/health | jq .otlp_logs_received

# Records should be searchable
mcporter call --config config/mcporter.json syslog-mcp.search 'service:claude-code' limit=5
```

---

## Rollback

* **rsyslog drop-ins (Phase 3, 4):** delete the `.conf` from
  `/etc/rsyslog.d/` and restart rsyslog. No data loss; future events stop
  flowing.
* **OTel client config (Phase 5):** unset the env keys (Claude) or remove
  the `[otel]` block (Codex). Already-stored records remain.
* **OTLP receiver (Phase 1):** revert the syslog-mcp container to a
  pre-0.11 image. The receiver routes simply disappear; existing syslog
  ingest continues unaffected.
* **Migration v3 (Phase 2):** the composite index can be dropped with
  `DROP INDEX idx_logs_app_name_received_at` if it causes any issue.
  Tag-based retention purges become slower but otherwise still correct.
