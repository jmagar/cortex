# Cortex Security Model

This document collects operator-facing trust assumptions that are otherwise
spread across the code and setup docs.

## Surfaces

| Surface | Default | Trust boundary |
| --- | --- | --- |
| Syslog UDP/TCP `:1514` | unauthenticated | any reachable sender can submit frames; restrict by bind address, firewall, or `CORTEX_ALLOWED_SOURCE_CIDRS` |
| MCP HTTP `/mcp` | bearer auth when `CORTEX_TOKEN` is set | `cortex:read` for read actions, `cortex:admin` for write/admin actions |
| OAuth/JWT | disabled unless `CORTEX_AUTH_MODE=oauth` | Google identity plus the configured cortex allowlist; static token is disabled by default in OAuth mode |
| OTLP `/v1/logs` | loopback or bearer-token protected | OAuth JWTs do not authorize OTLP ingest today |
| Docker ingest | disabled unless configured | trust the docker-socket-proxy host and private network path; proxy must be read-only |
| SSH inventory/deploy | disabled unless hosts are configured | inventory and remote Docker events use validated host aliases, strict host keys, shared concurrency limits, and retry backoff; deploy uses the same host validation, `--` delimiter, and host-key argument policy |

## Auth Scopes

The current public scopes are `cortex:read` and `cortex:admin`.
`cortex:admin` satisfies `cortex:read`. Static bearer tokens receive
`cortex:read` by default; set `CORTEX_STATIC_TOKEN_ADMIN=true` only for
operators that need `ack_error`, `unack_error`, or `notifications_test`.

## OAuth Platform Assumption

OAuth/JWT key and database file permission checks are Linux/Unix oriented.
On non-Unix platforms cortex fails closed instead of silently accepting weaker
ACL validation. Treat OAuth mode as Linux-only unless non-Unix ACL validation is
implemented and tested.

## SSH Host Keys

Inventory and deploy SSH default to `StrictHostKeyChecking=yes`. Bootstrap TOFU
requires explicit opt-in with `CORTEX_INVENTORY_SSH_TRUST_ON_FIRST_USE=true`.
Use `CORTEX_INVENTORY_SSH_KNOWN_HOSTS` to point cortex at a managed known-hosts
file for fleet automation.

## Identity Fields

Syslog `hostname` is sender-claimed. For UniFi CEF messages, the stored
`hostname` comes from `UNIFIdeviceName` in the message body. `source_ip` is the
network-observed source identifier and is the better trust boundary for
correlation and inventory decisions.

## Redaction Limits

cortex redacts known credential-looking environment keys and sensitive setup
values before persisting inventory artifacts. Redaction is defensive, not a
formal data-loss-prevention guarantee. Treat log messages, transcript text,
paths, Docker metadata keys, and source-specific `metadata_json` as sensitive
operator data.

## Dependency Exceptions

`cargo deny check` ignores `RUSTSEC-2023-0071` for transitive RSA usage through
`lab-auth -> jsonwebtoken -> rsa`. The accepted path is JWT signing and
verification, not PKCS#1 v1.5 decryption. The owner is the cortex maintainer;
review the exception every release and remove it when `lab-auth` or
`jsonwebtoken` moves to a hardened dependency path.

`cargo deny` also allows duplicate crate versions and wildcard git dependency
metadata because the current dependency graph includes transitive MCP/auth and
platform-target duplication plus a pinned `lab-auth` git revision. Source
allowlists, license policy, yanked crates, and advisory checks remain enforced.
