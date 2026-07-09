# cortex-rmcp

Node launcher for the `cortex` Rust MCP server, CLI, and homelab intelligence binary.

```bash
npx -y cortex-rmcp --help
```

The package downloads the matching GitHub Release binary during `postinstall`.

## MCP stdio

```json
{
  "mcpServers": {
    "cortex": {
      "command": "npx",
      "args": ["-y", "cortex-rmcp", "mcp"]
    }
  }
}
```

## Environment

- `CORTEX_RMCP_BINARY_VERSION`: release tag/version to download, defaulting to this npm package version.
- `CORTEX_RMCP_VERSION`: alias for `CORTEX_RMCP_BINARY_VERSION`.
- `CORTEX_RMCP_REPO`: GitHub `owner/repo`, defaulting to `jmagar/cortex`.
- `CORTEX_RMCP_RELEASE_BASE_URL`: full release download base URL.
- `CORTEX_RMCP_SKIP_DOWNLOAD=1`: skip postinstall download.
