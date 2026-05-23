# `bin/`

Holds the compiled `syslog` binary that Claude Code plugin installs put on `PATH`.

## How it gets here

`just build-plugin` runs `cargo build --release` and copies `target/release/syslog`
into this directory. Plugin marketplaces (Claude/Codex/Gemini) reference this
path for shell distribution.

## Contract

- Put executable entrypoints here, not repo-maintenance scripts
- Keep filenames stable and descriptive so they are safe to expose on `PATH`
- Each executable should have a shebang (or be a compiled binary)
- Commands should prefer deterministic behavior and clear exit codes

## Notes for Claude Code Plugins

This subtree is the plugin surface Claude Code invokes directly from the shell:

- the `syslog` CLI/MCP binary (primary)
- setup, validation, and wrapper helpers as needed
