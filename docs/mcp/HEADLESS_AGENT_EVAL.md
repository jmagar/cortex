# Headless Agent Prompt Evaluation

syslog-mcp already ingests Claude, Codex, and Gemini sessions, so prompt
evaluation should prefer live agent runs over a synthetic-only harness.

The practical shape is:

1. Render a prompt from the running MCP server with `prompts/get`.
2. Let a headless agent run the prompt against the same syslog MCP server it
   normally uses.
3. Ask the agent to return JSON conforming to `syslog://schema/prompt-output`.
4. Score the final shape and, when session ingestion is enabled, inspect the
   recorded session for action order, bounds, and evidence-before-claim
   behavior.

Use `scripts/prompt-headless-eval.sh` for the live path:

```bash
scripts/prompt-headless-eval.sh --dry-run --prompt infra.service-outage --arg service=plex
scripts/prompt-headless-eval.sh --agent codex --prompt infra.after-deploy-check --arg service=syslog-mcp
scripts/prompt-headless-eval.sh --agent claude --mcp-config /path/to/claude-mcp-config.json --prompt infra.storage-pressure
```

## Claude Code Headless Surface

Live `claude --help` plus Axon-backed docs identify `claude -p` /
`claude --print` as the non-interactive mode.

Important headless flags:

- `-p`, `--print`: print a response and exit.
- `--output-format text|json|stream-json`: choose final or streaming output.
- `--input-format text|stream-json`: use text or streaming JSON input.
- `--json-schema <schema>`: validate structured output.
- `--include-partial-messages`: include partial chunks with `stream-json`.
- `--include-hook-events`: include hook lifecycle events with `stream-json`.
- `--no-session-persistence`: avoid writing session files.
- `--session-id <uuid>`, `--resume`, `--continue`, `--fork-session`: control
  session identity and resumption.
- `--bare`: skip hooks, LSP, plugin sync, attribution, auto-memory,
  background prefetches, keychain reads, and CLAUDE.md discovery. Useful for
  deterministic CI runs when all context is supplied explicitly.
- `--mcp-config <file-or-json>` and `--strict-mcp-config`: control the MCP
  surface available to the run.
- `--tools`, `--allowedTools`, `--disallowedTools`: bound tool access.
- `--permission-mode default|acceptEdits|auto|bypassPermissions|dontAsk|plan`:
  control approval behavior.
- `--system-prompt`, `--append-system-prompt`, `--settings`, `--plugin-dir`,
  `--plugin-url`, `--model`, `--effort`, `--max-budget-usd`: control runtime
  policy, plugins, model, and budget.

## Codex Headless Surface

Live `codex --help` and `codex exec --help` identify `codex exec` as the
non-interactive headless mode.

Important headless flags:

- `codex exec [PROMPT]`: run non-interactively; reads stdin when prompt is
  omitted or `-` is used.
- `--json`: print agent events to stdout as JSONL.
- `--output-schema <file>`: require the final response to match a JSON Schema.
- `--output-last-message <file>`: write the final assistant message to a file.
- `--ephemeral`: run without persisting session files.
- `--ignore-user-config`, `--ignore-rules`, `--strict-config`: make CI runs
  more deterministic.
- `--cd <dir>`, `--add-dir <dir>`: control workspace access.
- `--sandbox read-only|workspace-write|danger-full-access`: set command
  sandboxing.
- `--dangerously-bypass-approvals-and-sandbox`: use only inside an external
  sandbox.
- `--model`, `--profile`, `--profile-v2`, `--config key=value`, `--enable`,
  `--disable`: control model and configuration.
- `--image <file>`: attach images to the initial prompt.
- `codex review`: non-interactive code review mode, with `--uncommitted`,
  `--base`, `--commit`, and `--title`.

## Why This Beats a Pure Fixture Harness

Synthetic fixtures are still useful for CI shape checks, but they cannot prove
that agents use the live MCP surface efficiently. Headless Claude/Codex runs can
exercise the real server, real auth, real session ingestion, and real tool
schemas. The deterministic part should stay narrow: prompt rendering, output
schema validation, known action references, bounded-call guidance, and invalid
parameter regressions.
