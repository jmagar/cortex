# Cortex Documentation

Complete documentation for `cortex` -- a Rust syslog receiver and MCP server for homelab log intelligence.

## Directory index

### Authoritative current docs

| File | Purpose |
| --- | --- |
| `README.md` | This file -- documentation index |
| `SETUP.md` | Step-by-step setup guide -- clone, build, configure, deploy, verify (at `docs/SETUP.md`) |
| `CONFIG.md` | Configuration reference -- config.toml, env vars, storage budget |
| `CLI.md` | Direct CLI reference -- local search, tail, errors, hosts, correlate, and stats commands |
| `api.md` | REST API endpoint matrix (22 routes), versioning, perf, threat model, response caps, VACUUM caveats |
| `architecture.md` | Caller → DB diagram (HTTP CLI default + direct-SQLite consumers) |
| `rollout.md` | Manual v0.26 upgrade playbook for HTTP CLI cutover |
| `CHECKLIST.md` | Pre-release quality checklist -- version sync, security, CI, registry |
| `GUARDRAILS.md` | Security guardrails -- credentials, Docker, auth, input handling |
| `INVENTORY.md` | Component inventory -- tools, env vars, surfaces, dependencies |
| `OAUTH.md` | OAuth/JWT operator configuration and runtime model |
| `RUST.md` | Rust toolchain and rmcp dependency intent |
| `SECURITY.md` | Consolidated operator trust model |
| `RELEASE.md` | Release gates: hermetic CI versus live fleet checks |

### Subdirectories

| Directory | Scope |
| --- | --- |
| `mcp/` | MCP server docs: auth, transport, tools, resources, testing, deployment |
| `plugin/` | Plugin system docs: manifests, hooks, skills, commands, channels |
| `repo/` | Repository docs: git conventions, scripts, memory, rules |
| `stack/` | Technology stack docs: prerequisites, architecture, Rust dependencies |
| `upstream/` | Upstream service docs (cortex is self-contained -- no external API) |

### Preserved and archival directories

Files in these directories are useful historical context, but they are not the
source of truth for current command names, plugin paths, auth scopes, release
version policy, or install examples. Prefer the authoritative docs above for
operator instructions.

| Directory | Scope |
| --- | --- |
| `plans/` | Engineering plans and design docs |
| `runbooks/` | Operational runbooks (deploy, maintenance) |
| `sessions/` | Development session notes |
| `superpowers/` | Superpowers plans (storage budget guardrail, etc.) |

## Cross-references

- [CLAUDE.md](../CLAUDE.md) -- project instructions for Claude Code sessions
- [README.md](../README.md) -- user-facing project overview
- [CLI.md](CLI.md) -- direct local CLI command reference
- [SETUP.md](SETUP.md) -- host configuration guide (rsyslog, UniFi, ATT router)
- [CHANGELOG.md](../CHANGELOG.md) -- version history
