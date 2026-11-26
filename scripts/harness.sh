#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Ash-Net: New Lawrence — Zentraler Selbstverifizierungs-Harness für KI-Agenten & CI

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CURRENT_SPRINT="${CURRENT_SPRINT:-1}"
ARTIFACTS_DIR="${ARTIFACTS_DIR:-$ROOT_DIR/.harness-artifacts}"
COMPOSE_PROJECT="ash-harness"
COMPOSE_FILE="$ROOT_DIR/docker-compose.harness.yml"
HARNESS_BOOT_TIMEOUT_SECS="${HARNESS_BOOT_TIMEOUT_SECS:-90}"

mkdir -p "$ARTIFACTS_DIR"

usage() {
    cat <<'EOF'
Usage: ./scripts/harness.sh <command> [target]

Commands:
  gate                Führt ./scripts/check-invariants.sh aus (SPDX, 3-Zonen-Graph, Governance, fmt, clippy, tests)
  sim [pfad|all]      Führt deterministische ash-core-Simulations-Szenarien (mode = "sim") aus
  e2e [pfad|smoke]    Startet ephemeren Docker-Stack (ash-harness), prüft Health & führt E2E-Szenario aus
  all                 Führt nacheinander gate -> sim -> e2e aus
EOF
}

run_gate() {
    echo ">>> [HARNESS:GATE] Starte ./scripts/check-invariants.sh ..."
    ./scripts/check-invariants.sh
}

run_sim() {
    local target="${1:-all}"
    local files=()

    if [[ "$target" == "all" ]]; then
        while IFS= read -r f; do
            if grep -Eq '^[[:space:]]*mode[[:space:]]*=[[:space:]]*"sim"' "$f"; then
                files+=("$f")
            fi
        done < <(find harness/scenarios -maxdepth 1 -name "*.toml" -type f | sort)
    else
        files+=("$target")
    fi

    if [[ "${#files[@]}" -eq 0 ]]; then
        echo "ERROR: Keine sim-Szenarien unter harness/scenarios/ gefunden." >&2
        exit 1
    fi

    echo ">>> [HARNESS:SIM] Prüfe ${#files[@]} Szenario(en) (CURRENT_SPRINT=$CURRENT_SPRINT)..."
    cargo build --quiet -p ash-test-cli
    local cli_bin
    cli_bin="$(cargo metadata --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')/debug/ash-test-cli"

    for scen in "${files[@]}"; do
        local base
        base="$(basename "$scen" .toml)"
        local trace_out="$ARTIFACTS_DIR/${base}.ndjson"
        local report_out="$ARTIFACTS_DIR/${base}.md"

        if ! "$cli_bin" run-scenario "$scen" \
            --current-sprint "$CURRENT_SPRINT" \
            --trace "$trace_out" \
            --report "$report_out"; then
            cp -f "$report_out" "$ARTIFACTS_DIR/last-failure.md" 2>/dev/null || true
            echo ">>> [HARNESS:SIM] FEHLER in '$scen' — kompakter Bericht in $ARTIFACTS_DIR/last-failure.md" >&2
            exit 1
        fi
    done

    echo ">>> [HARNESS:SIM] Alle aktiven Simulations-Szenarien erfolgreich (Traces in .harness-artifacts/)."
}

compose_cmd() {
    docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" "$@"
}

cleanup_e2e_stack() {
    echo ">>> [HARNESS:E2E] Räume ephemeren Stack '$COMPOSE_PROJECT' restlos ab (down -v --remove-orphans)..."
    compose_cmd down -v --remove-orphans >/dev/null 2>&1 || true
}

write_compact_failure_digest() {
    local exit_code="$1"
    local scenario_file="$2"
    local cli_report="$3"
    local digest="$ARTIFACTS_DIR/last-failure.md"

    {
        echo "# Ash-Net Harness — Kompakter Fehler-Digest"
        echo ""
        echo "- **Szenario:** \`$scenario_file\`"
        echo "- **Exit-Code:** \`$exit_code\`"
        echo "- **Sprint-Level:** \`CURRENT_SPRINT=$CURRENT_SPRINT\`"
        echo ""
        if [[ -s "$cli_report" ]]; then
            echo "## 1. Strukturierter Szenario-Report (\`ash-test-cli\`)"
            cat "$cli_report"
            echo ""
        fi

        echo "## 2. Container-Status (\`docker compose ps\`)"
        echo '```text'
        compose_cmd ps 2>&1 || true
        echo '```'
        echo ""

        echo "## 3. Gefilterte Warn-/Fehler-Signale (\`ERROR|WARN|panic|ASSERTION|FAILED_STEP\`)"
        echo '```text'
        compose_cmd logs --no-color 2>&1 \
            | grep -Ei 'ERROR|WARN|panic|ASSERTION|FAILED_STEP' \
            | tail -n 60 || echo "(keine passenden Fehlerzeilen)"
        echo '```'
        echo ""

        echo "## 4. Letzte Log-Zeilen (max. 30 Zeilen pro Kern-Container)"
        for svc in auth-gateway game-node-roadhouse game-node-frontier irc-bridge ircd; do
            echo "### Service: \`$svc\`"
            echo '```text'
            compose_cmd logs --no-color --tail=30 "$svc" 2>&1 || true
            echo '```'
        done
    } >"$digest"

    echo "" >&2
    cat "$digest" >&2
}

wait_for_core_services_healthy() {
    local core_services=(
        postgres
        redis
        ircd
        auth-gateway
        game-node-roadhouse
        game-node-frontier
        irc-bridge
    )
    local deadline=$((SECONDS + HARNESS_BOOT_TIMEOUT_SECS))

    echo ">>> [HARNESS:E2E] Warte auf Status 'healthy' für ${#core_services[@]} Services (Timeout: ${HARNESS_BOOT_TIMEOUT_SECS}s)..."
    while (( SECONDS < deadline )); do
        local all_healthy=1
        for svc in "${core_services[@]}"; do
            local cid
            cid="$(compose_cmd ps -q "$svc" 2>/dev/null || true)"
            if [[ -z "$cid" ]]; then
                all_healthy=0
                break
            fi
            local health
            health="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$cid" 2>/dev/null || echo "unknown")"
            if [[ "$health" != "healthy" ]]; then
                all_healthy=0
                break
            fi
        done

        if [[ "$all_healthy" -eq 1 ]]; then
            # Kurzes Fenster für initialen mTLS-Gossip-Sync & IRC-Channel-Joins
            sleep 1
            echo ">>> [HARNESS:E2E] Alle Core-Services sind healthy."
            return 0
        fi
        sleep 1
    done

    echo "ERROR: Timeout beim Warten auf healthy Core-Services." >&2
    return 1
}

run_e2e() {
    local target="${1:-smoke}"
    local scenario_rel
    if [[ "$target" == "smoke" ]]; then
        scenario_rel="harness/scenarios/00-smoke-mesh.toml"
    else
        scenario_rel="$target"
    fi

    if [[ ! -f "$scenario_rel" ]]; then
        echo "ERROR: E2E-Szenario '$scenario_rel' existiert nicht." >&2
        exit 1
    fi

    if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
        echo "ERROR: Kein laufender Docker-Daemon verfügbar — './scripts/harness.sh e2e' benötigt Docker Compose." >&2
        echo "TIPP: Für deterministische Prüfungen ohne Docker './scripts/harness.sh gate && ./scripts/harness.sh sim' nutzen." >&2
        exit 1
    fi

    local base
    base="$(basename "$scenario_rel" .toml)"
    local host_report="$ARTIFACTS_DIR/${base}.md"
    local container_scen="/${scenario_rel}"

    trap cleanup_e2e_stack EXIT INT TERM

    echo ">>> [HARNESS:E2E] Starte ephemeren Stack '$COMPOSE_PROJECT' ($COMPOSE_FILE)..."
    compose_cmd up --build -d

    if ! wait_for_core_services_healthy; then
        write_compact_failure_digest 124 "$scenario_rel" "$host_report"
        exit 124
    fi

    echo ">>> [HARNESS:E2E] Führe Szenario '$scenario_rel' via ash-test-cli aus..."
    set +e
    compose_cmd --profile runner run --rm ash-test-cli \
        run-scenario "$container_scen" \
        --current-sprint "$CURRENT_SPRINT" \
        --trace "/artifacts/${base}.ndjson" \
        --report "/artifacts/${base}.md"
    local rc=$?
    set -e

    if [[ "$rc" -ne 0 ]]; then
        write_compact_failure_digest "$rc" "$scenario_rel" "$host_report"
        exit "$rc"
    fi

    echo ">>> [HARNESS:E2E] Szenario '$scenario_rel' erfolgreich bestanden!"
}

CMD="${1:-}"
SHIFT_ARG="${2:-}"

case "$CMD" in
    gate)
        run_gate
        ;;
    sim)
        run_sim "${SHIFT_ARG:-all}"
        ;;
    e2e)
        run_e2e "${SHIFT_ARG:-smoke}"
        ;;
    all)
        run_gate
        run_sim "all"
        run_e2e "smoke"
        ;;
    *)
        usage
        exit 1
        ;;
esac
