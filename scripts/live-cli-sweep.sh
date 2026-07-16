#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/target/live-cli-sweep}"
mkdir -p "$OUT_DIR"

SUMMARY="$OUT_DIR/summary.tsv"
: >"$SUMMARY"
printf "name\tmode\texit\tseconds\tlog\n" >>"$SUMMARY"

ENV_FILE="${CORTEX_ENV_FILE:-$HOME/.cortex/.env}"
SERVER="${CORTEX_URL:-http://127.0.0.1:3100}"
TIMEOUT="${CORTEX_SWEEP_TIMEOUT:-120}"

run_case() {
  local name="$1"
  local mode="$2"
  local cmd="$3"
  local safe_name log start end status
  safe_name="$(printf '%s' "$name" | tr -c 'A-Za-z0-9_.-' '_')"
  log="$OUT_DIR/$safe_name.log"
  start="$(date +%s)"
  (
    cd "$ROOT" || exit 99
    set -a
    if [[ -f "$ENV_FILE" ]]; then
      # shellcheck source=/dev/null
      source "$ENV_FILE"
    fi
    set +a
    export CORTEX_USE_HTTP=true
    export CORTEX_URL="$SERVER"
    export CORTEX_SWEEP_TMP="${CORTEX_SWEEP_TMP:-$OUT_DIR/tmp}"
    mkdir -p "$CORTEX_SWEEP_TMP"
    timeout "$TIMEOUT" bash -lc "$cmd"
  ) >"$log" 2>&1
  status=$?
  end="$(date +%s)"
  printf "%s\t%s\t%s\t%s\t%s\n" "$name" "$mode" "$status" "$((end-start))" "$log" >>"$SUMMARY"
}

run_case "top.version" "read" "cortex --version"
run_case "top.help" "read" "cortex --help"
run_case "nested.help.sessions" "read" "cortex sessions --help"
run_case "nested.help.db" "read" "cortex db --help"
run_case "completion.zsh" "read" "cortex completions zsh >/dev/null"
run_case "complete.top" "read" "cortex __complete actions"

run_case "search" "read" "cortex search --json --grep cortex --limit 1"
run_case "filter" "read" "cortex filter --json --limit 1"
run_case "tail" "read" "cortex tail --json -n 1"
run_case "hosts.list" "read" "cortex hosts --json"
run_case "hosts.sources" "read" "cortex hosts sources --json --limit 5"
run_case "hosts.silent" "read" "cortex hosts silent --json --silent-minutes 1"
run_case "apps" "read" "cortex apps --json --limit 5"
run_case "entity" "read" "cortex entity host dookie --json"
run_case "graph.status" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex graph status --json"
run_case "graph.around" "read" "cortex graph around host dookie --json --limit 5"
run_case "graph.explain" "read" "cortex graph explain host dookie --json --max-chains 5"
run_case "graph.evidence.expected-arg" "parse" "cortex graph evidence 1 --json"
run_case "graph.rebuild" "local-mutation" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex graph rebuild --json"

run_case "analysis.errors" "read" "cortex analysis errors --json --since 1h --limit 5"
run_case "analysis.incident" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex analysis incident --json --around now --service cortex --minutes 5 --limit 5"
run_case "analysis.patterns" "read" "cortex analysis patterns --json --since 1h --limit 5"
run_case "analysis.anomalies" "read" "cortex analysis anomalies --json --recent-minutes 60 --baseline-minutes 120"
run_case "analysis.compare" "read" "cortex analysis compare --json --a-from 2h --a-to 1h --b-from 1h --b-to now"
run_case "correlate.events" "read" "cortex correlate events --json --query cortex --window-minutes 5 --limit 5"
run_case "correlate.state" "read" "cortex correlate state --json --reference-time now --host dookie --window-minutes 5 --limit 5"
run_case "correlate.topic" "read" "cortex correlate topic --json cortex --since 1h --limit 5"
run_case "state.host" "read" "cortex state host localhost --json"
run_case "state.fleet" "read" "cortex state fleet --json"
run_case "state.clock-skew" "read" "cortex state clock-skew --json --limit 5"
run_case "stats.summary" "read" "cortex stats summary --json"
run_case "stats.ingest-rate" "read" "cortex stats ingest-rate --json"
run_case "status" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex status --json"
run_case "timeline" "read" "cortex timeline --json --since 1h"

run_case "sessions.list" "read" "cortex sessions --json --limit 2"
run_case "sessions.search" "read" "cortex sessions search --json cortex --limit 2"
run_case "sessions.abuse" "read" "cortex sessions abuse --json --limit 2"
run_case "sessions.correlate" "read" "cortex sessions correlate --json --ai-query cortex --limit 2 --events-per-anchor 1"
run_case "sessions.blocks" "read" "cortex sessions blocks --json --limit 2"
run_case "sessions.context" "read" "cortex sessions context --json /home/jmagar/workspace/cortex --limit 2"
run_case "sessions.tools" "read" "cortex sessions tools --json"
run_case "sessions.projects" "read" "cortex sessions projects --json"
run_case "sessions.checkpoints" "read" "cortex sessions checkpoints --json --limit 2"
run_case "sessions.errors" "read" "cortex sessions errors --json --limit 2"
run_case "sessions.prune-checkpoints.dry-run" "admin" "cortex sessions prune-checkpoints --json --missing --dry-run --limit 2"
run_case "sessions.doctor" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions doctor --json"
run_case "sessions.watch-status" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions watch-status --json"
run_case "sessions.smoke-watch" "local-mutation" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions smoke-watch --json"
run_case "sessions.similar" "read" "cortex sessions similar --json cortex --limit 2"
run_case "sessions.incident-context" "read" "cortex sessions incident-context --json --since 1h --until now --limit 2"
run_case "sessions.incidents" "read" "cortex sessions incidents --json --limit 2"
run_case "sessions.investigate" "read" "cortex sessions investigate --json --limit 1 --max-bytes 2048"
run_case "sessions.assess.expected-empty" "expected-fail" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions assess nonexistent-incident --json --dry-run"
run_case "sessions.llm-invocations" "admin" "cortex sessions llm-invocations --json --limit 2"
run_case "sessions.skills" "read" "cortex sessions skills --json --limit 2"
run_case "sessions.skills.backfill.dry-run" "local-mutation-dry-run" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions skills backfill --json --dry-run --limit 2"
run_case "sessions.skill-incidents" "read" "cortex sessions skill-incidents --json --limit 2"
run_case "sessions.skill-investigate" "read" "cortex sessions skill-investigate imagegen --json --limit 1"
run_case "sessions.skill-assess" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions skill-assess imagegen --json --no-llm --limit 1"
run_case "sessions.mcp-events" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions mcp-events --json --limit 2"
run_case "sessions.mcp-events.backfill.dry-run" "local-mutation-dry-run" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions mcp-events backfill --json --dry-run --limit 2"
run_case "sessions.mcp-incidents" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions mcp-incidents --json --limit 2"
run_case "sessions.mcp-investigate" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions mcp-investigate labby --json --limit 1"
run_case "sessions.mcp-assess" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions mcp-assess labby --json --no-llm --limit 1"
run_case "sessions.hook-events" "read" "cortex sessions hook-events --json --limit 2"
run_case "sessions.hooks-backfill.dry-run" "local-mutation-dry-run" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex sessions hooks-backfill --json --dry-run --limit 2"

run_case "assess.skill" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex assess skill imagegen --json --no-llm --limit 1"
run_case "assess.abuse" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex assess abuse --json --no-llm --limit 1"
run_case "assess.mcp" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex assess mcp labby --json --no-llm --limit 1"
run_case "assess.hooks" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex assess hooks --json --no-llm --limit 1"

run_case "alerts.signatures.list" "read" "cortex alerts signatures list --json --limit 2"
run_case "alerts.signatures.ack-unack" "admin" 'hash="$(cortex alerts signatures list --json --limit 1 | python3 -c "import json,sys; d=json.load(sys.stdin); xs=d.get(\"signatures\") or d.get(\"items\") or d.get(\"results\") or []; print((xs[0].get(\"signature_hash\") or xs[0].get(\"hash\")) if xs else \"\")")"; test -n "$hash" || { echo "no signature available"; exit 0; }; cortex alerts signatures ack "$hash" --json --notes "live-cli-sweep"; cortex alerts signatures unack "$hash" --json --reason "live-cli-sweep revert"'
run_case "alerts.notifications.recent" "read" "cortex alerts notifications recent --json --limit 2"
run_case "alerts.notifications.test" "admin" "cortex alerts notifications test --json --body live-cli-sweep"

run_case "ingest.inventory.status" "read" "cortex ingest inventory status --json"
run_case "ingest.inventory.refresh" "local-mutation" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db CORTEX_INVENTORY_DIR=$HOME/.cortex/inventory; cortex ingest inventory refresh --json"
run_case "ingest.syslog.status" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex ingest syslog status --json"
run_case "ingest.syslog.test.expected-deferred" "expected-fail" "cortex ingest syslog test"
run_case "ingest.docker.status" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex ingest docker status --json"
run_case "ingest.docker.sources" "local-read" "export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; cortex ingest docker sources --json"
run_case "ingest.file-tail.list" "admin" "cortex ingest file-tail list --json"
run_case "ingest.file-tail.status" "admin" "cortex ingest file-tail status --json"
run_case "ingest.file-tail.add-toggle-remove" "admin" 'id="live-cli-sweep"; cortex ingest file-tail add --id "$id" --path /file-tail-root/auth.log --tag live-cli-sweep --host dookie --json; cortex ingest file-tail disable --id "$id" --json; cortex ingest file-tail enable --id "$id" --json; cortex ingest file-tail remove --id "$id" --json'
run_case "ingest.shell.user.index" "local-mutation" 'export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; hist="$CORTEX_SWEEP_TMP/history"; printf "echo live-cli-sweep\n" >"$hist"; cortex ingest shell user index --path "$hist" --shell zsh --json'
run_case "ingest.shell.user.atuin-index" "local-mutation" 'export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; atuin="$CORTEX_SWEEP_TMP/atuin.db"; rm -f "$atuin"; sqlite3 "$atuin" "CREATE TABLE history(id TEXT PRIMARY KEY, timestamp INTEGER, duration INTEGER, exit INTEGER, command TEXT, cwd TEXT, session TEXT, hostname TEXT, author TEXT, intent TEXT, deleted_at INTEGER); INSERT INTO history VALUES ('"'"'live-cli-sweep'"'"', 1770000000000000000, 1000000, 0, '"'"'echo live-cli-sweep'"'"', '"'"'/tmp'"'"', '"'"'sweep-session'"'"', '"'"'dookie'"'"', '"'"'codex'"'"', NULL, NULL);"; cortex ingest shell user atuin-index --path "$atuin" --json'
run_case "ingest.shell.agent.wrap-probe" "read" "cortex ingest shell agent wrap --probe"
run_case "ingest.shell.agent.index" "local-mutation" 'export CORTEX_USE_HTTP=false CORTEX_DB_PATH=$HOME/.cortex/data/cortex.db; spool="$CORTEX_SWEEP_TMP/agent-spool.jsonl"; : >"$spool"; chmod 600 "$spool"; cortex ingest shell agent index --path "$spool" --json'

run_case "heartbeat.agent.once.emit" "read" 'cortex heartbeat agent --once --emit --json --target "$CORTEX_URL" --token "$CORTEX_API_TOKEN" --docker'

run_case "db.status" "read" "cortex db status --json"
run_case "db.status.coord" "read" "cortex db status --json --check-coord"
run_case "db.integrity.quick" "read" "cortex db integrity --json --quick"
run_case "db.integrity.background" "admin" "cortex db integrity --json --quick --background"
run_case "db.checkpoint.passive" "admin" "cortex db checkpoint --json --mode passive"
run_case "db.vacuum.incremental" "admin" "cortex db vacuum --json --pages 1"
run_case "db.backup" "admin" 'cortex db backup --json --output "$CORTEX_SWEEP_TMP/cortex-backup.db"'

run_case "compose.status" "read" "cortex compose status --json"
run_case "compose.doctor" "read" "cortex compose doctor --json"
run_case "compose.logs" "read" "cortex compose logs --json --tail 5"
run_case "compose.logs.service" "read" "cortex compose logs cortex --json --tail 5"
run_case "compose.up.dry-run" "mutation-dry-run" "cortex compose up --json --dry-run"
run_case "compose.restart.dry-run" "mutation-dry-run" "cortex compose restart --json --dry-run"
run_case "compose.pull.dry-run" "mutation-dry-run" "cortex compose pull --json --dry-run"
run_case "compose.down.dry-run" "mutation-dry-run" "cortex compose down --json --dry-run --yes"
run_case "compose.config.expected-deferred" "expected-fail" "cortex compose config"
run_case "compose.upgrade.expected-deferred" "expected-fail" "cortex compose upgrade"

run_case "setup.check" "read" "cortex setup check --json"
run_case "setup.plugin-hook.no-repair" "read" "cortex setup plugin-hook --json --no-repair"
run_case "config.list" "read" 'cfg="$CORTEX_SWEEP_TMP/config.toml"; printf "[mcp]\nport = 3100\n" >"$cfg"; cortex config list --toml --toml-path "$cfg" --json'
run_case "config.get" "read" 'cfg="$CORTEX_SWEEP_TMP/config.toml"; printf "[mcp]\nport = 3100\n" >"$cfg"; cortex config get mcp.port --toml --toml-path "$cfg" --json'
run_case "config.set" "mutation-temp" 'cfg="$CORTEX_SWEEP_TMP/config.toml"; printf "[mcp]\nport = 3100\n" >"$cfg"; cortex config set test.value live-cli-sweep --toml --toml-path "$cfg" --json'
run_case "config.unset" "mutation-temp" 'cfg="$CORTEX_SWEEP_TMP/config.toml"; printf "[test]\nvalue = \"live-cli-sweep\"\n" >"$cfg"; cortex config unset test.value --toml --toml-path "$cfg" --json'

echo "summary: $SUMMARY"
awk -F '\t' 'NR == 1 { next } $3 != 0 { print }' "$SUMMARY" >"$OUT_DIR/failures.tsv"
echo "failures: $OUT_DIR/failures.tsv"
