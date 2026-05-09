# Hive Migration Guide

Hive is the new product name for `syslog-mcp`. The service still ingests RFC
syslog over UDP/TCP, but the package, binary, MCP, plugin, and deployment
identity now use Hive names.

## Name Mapping

| Surface | Old | New | Compatibility |
| --- | --- | --- | --- |
| Cargo package | `syslog-mcp` | `hive-mcp` | Package rename is breaking. |
| Rust crate import | `syslog_mcp` | `hive_mcp` | Import rename is breaking. |
| Primary binary | `syslog` | `hive` | `syslog` remains a binary alias this release. |
| MCP tool | `syslog` | `hive` | `syslog` remains callable this release. |
| Read scope | `syslog:read` | `hive:read` | Both are accepted this release. |
| Admin scope | `syslog:admin` | `hive:admin` | Both are accepted this release. |
| Schema resource | `syslog://schema/mcp-tool` | `hive://schema/mcp-tool` | Both are readable this release. |
| MCP env vars | `SYSLOG_MCP_*` | `HIVE_MCP_*` | `HIVE_MCP_*` wins when both are set. |
| API env vars | `SYSLOG_API_*` | `HIVE_API_*` | `HIVE_API_*` wins when both are set. |
| Docker ingest env vars | `SYSLOG_DOCKER_*` | `HIVE_DOCKER_*` | `HIVE_DOCKER_*` wins when both are set. |
| Syslog listener vars | `SYSLOG_HOST`, `SYSLOG_PORT`, `SYSLOG_*` ingest knobs | unchanged | These remain protocol-specific. |

## Docker Data Preservation

The default database path remains `/data/syslog.db` for this release. The file
name is treated as a data artifact, not product branding.

The compose file uses Hive service/image/container names. For data safety, an
explicit `HIVE_MCP_DATA_VOLUME` or legacy `SYSLOG_MCP_DATA_VOLUME` value is
respected. Existing operators using the old named volume can keep it:

```bash
HIVE_MCP_DATA_VOLUME=syslog-mcp-data docker compose up -d
```

Operators who intentionally want a new Hive-named volume can copy first:

```bash
docker volume create hive-mcp-data
docker run --rm \
  -v syslog-mcp-data:/from:ro \
  -v hive-mcp-data:/to \
  alpine sh -c 'cd /from && cp -a . /to/'
HIVE_MCP_DATA_VOLUME=hive-mcp-data docker compose up -d
```

Run the upgrade check before changing production compose settings:

```bash
bash scripts/verify-compose-upgrade.sh
```

## MCP Client Update

New clients should call tool `hive`:

```json
{"name": "hive", "arguments": {"action": "stats"}}
```

Existing clients using tool `syslog` continue to work during this transition
release. New OAuth/JWT clients should request `hive:read` or `hive:admin`.
Legacy `syslog:read` and `syslog:admin` remain accepted for compatibility.

## Environment Update

Prefer Hive variables for product-level settings:

```bash
HIVE_MCP_TOKEN=...
HIVE_MCP_HOST=0.0.0.0
HIVE_MCP_PORT=3100
HIVE_MCP_DB_PATH=/data/syslog.db
HIVE_DOCKER_INGEST_ENABLED=false
```

Keep syslog protocol listener variables as-is:

```bash
SYSLOG_HOST=0.0.0.0
SYSLOG_PORT=1514
SYSLOG_BATCH_SIZE=100
SYSLOG_FLUSH_INTERVAL=500
```
