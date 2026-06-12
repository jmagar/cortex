# Release Checklist

Use this checklist before merging release-bound work. CI covers hermetic gates;
live fleet gates require a running cortex deployment and explicit operator
intent.

## Hermetic Gates

Run from the repo root:

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo deny check
bash scripts/check-version-sync.sh
bash scripts/check-plugin-manifest-versions.sh
bash scripts/check-agent-memory-symlinks.sh
bash scripts/check-public-identity.sh
git diff --check
```

For release commits, also require:

```bash
bash scripts/check-version-sync.sh --require-changelog
```

Version-bearing files are `Cargo.toml`, `server.json`, `mcpb/manifest.json`,
`Cargo.lock`, and `CHANGELOG.md`. Plugin manifests are intentionally
unversioned.

## Live Gates

Run these only against an intended test or production deployment:

```bash
bash tests/test_live.sh
bash scripts/smoke-test.sh
bash scripts/smoke-test-http.sh
bash tests/mcporter/test-tools.sh
```

Live Docker ingest validation now follows two paths: host-local cortex agent
parity for deployed agents, and the legacy central pull fixture with
`CORTEX_DOCKER_INGEST_ENABLED=true` plus `CORTEX_DOCKER_HOSTS` set to an
explicit Docker-compatible HTTP endpoint.

Live SSH inventory validation requires configured SSH aliases or
`CORTEX_INVENTORY_SSH_HOSTS`, strict known-hosts coverage, and any intentional
TOFU bootstrap set explicitly with `CORTEX_INVENTORY_SSH_TRUST_ON_FIRST_USE=true`.

Fleet drop-in deployment is intentionally outside hermetic CI. Validate first
with:

```bash
cortex compose doctor
cortex inventory refresh --json
```

Then use the `cortex-deploy-dropins` plugin skill or the documented deploy
workflow only when the target `fleet_hosts` list is correct and reachable.

## Commit Policy

Every feature branch push bumps the version according to the repo policy in
`CLAUDE.md`. Patch bumps are appropriate for fixes, docs, CI, test, and policy
work. `CHANGELOG.md` must describe the operator-visible behavior, not just the
file list.
