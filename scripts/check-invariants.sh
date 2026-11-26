#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Ash-Net: New Lawrence — Maschinelles Architektur-, Lizenz- & Governance-Gate (Säule C)

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

OVERALL_FAILED=0

echo "========================================================================"
echo " ASH-NET: NEW LAWRENCE — INVARIANT & 3-PILLAR BOUNDARY GATE"
echo "========================================================================"

# ------------------------------------------------------------------------------
# Check 1: SPDX-Lizenz-Header in allen .rs-Dateien (außer target/)
# ------------------------------------------------------------------------------
echo ""
echo "[Check 1/4] Prüfe SPDX-License-Identifier (GPL-3.0-or-later) in allen .rs-Dateien..."
SPDX_FAILED=0
RS_COUNT=0

while IFS= read -r -d '' rs_file; do
    RS_COUNT=$((RS_COUNT + 1))
    if ! head -n 5 "$rs_file" | grep -q "SPDX-License-Identifier: GPL-3.0-or-later"; then
        echo "  SPDX VIOLATION: fehlender GPL-3.0-or-later Header in '$rs_file'"
        SPDX_FAILED=1
    fi
done < <(find . -path "./target" -prune -o -name "*.rs" -type f -print0 | sort -z)

if [[ "$SPDX_FAILED" -eq 0 ]]; then
    echo "  -> OK ($RS_COUNT .rs-Dateien geprüft)."
else
    echo "  -> FEHLGESCHLAGEN: Mindestens eine .rs-Datei verletzt die SPDX-Pflicht."
    OVERALL_FAILED=1
fi

# ------------------------------------------------------------------------------
# Check 2: Maschinelle 3-Zonen-Abhängigkeitsgraph-Prüfung (cargo metadata + Python3)
# ------------------------------------------------------------------------------
echo ""
echo "[Check 2/4] Prüfe 3-Zonen-Architektur (SHARED / SERVER / CLIENT) via cargo metadata..."

if ! python3 - <<'PY'
import collections
import json
import subprocess
import sys

raw_meta = subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1"],
    text=True,
)
data = json.loads(raw_meta)
workspace_members = set(data["workspace_members"])
packages_by_id = {pkg["id"]: pkg for pkg in data["packages"]}

VALID_ZONES = {"shared", "server", "client"}
SHARED_FORBIDDEN_CRATES = {"tokio", "quinn", "sqlx", "redis", "ratatui", "crossterm"}
SERVER_FORBIDDEN_CRATES = {"ratatui", "crossterm"}
CLIENT_FORBIDDEN_CRATES = {"sqlx", "redis", "hecs", "bevy_ecs"}

ws_zones = {}
violations = []

# 1. Zonen-Deklarationspflicht prüfen
for pkg_id in sorted(workspace_members):
    pkg = packages_by_id[pkg_id]
    name = pkg["name"]
    meta = pkg.get("metadata") or {}
    ash_meta = meta.get("ash") if isinstance(meta, dict) else None
    zone = ash_meta.get("zone") if isinstance(ash_meta, dict) else None
    if zone not in VALID_ZONES:
        violations.append(
            f"ZONE DECLARATION VIOLATION: workspace crate '{name}' must define "
            f"[package.metadata.ash] zone in {sorted(VALID_ZONES)} (found: {zone!r})"
        )
    else:
        ws_zones[pkg_id] = zone

print("  Erkannte Workspace-Zonen:")
for pkg_id in sorted(workspace_members, key=lambda pid: packages_by_id[pid]["name"]):
    pkg = packages_by_id[pkg_id]
    z = ws_zones.get(pkg_id, "<INVALID>")
    print(f"    - {pkg['name']:20s} -> zone = {z}")

# 2. Gerichteten Abhängigkeitsgraphen aufbauen (normal + build strikt getrennt von reinen dev-deps)
normal_build_adj = collections.defaultdict(list)
dev_only_adj = collections.defaultdict(list)

resolve = data.get("resolve") or {}
for node in resolve.get("nodes", []):
    src_id = node["id"]
    for dep in node.get("deps", []):
        dst_id = dep["pkg"]
        dep_kinds = dep.get("dep_kinds") or []
        if not dep_kinds:
            normal_build_adj[src_id].append(dst_id)
            continue
        has_normal_or_build = any(dk.get("kind") in (None, "build") for dk in dep_kinds)
        has_dev = any(dk.get("kind") == "dev" for dk in dep_kinds)
        if has_normal_or_build:
            normal_build_adj[src_id].append(dst_id)
        elif has_dev:
            dev_only_adj[src_id].append(dst_id)

def bfs_shortest_paths(start_id):
    """Liefert {reachable_pkg_id: [pkg_id_chain]} über normal+build-Abhängigkeiten."""
    paths = {start_id: [start_id]}
    queue = collections.deque([start_id])
    while queue:
        curr = queue.popleft()
        for nxt in sorted(normal_build_adj.get(curr, []), key=lambda pid: packages_by_id[pid]["name"]):
            if nxt not in paths:
                paths[nxt] = paths[curr] + [nxt]
                queue.append(nxt)
    return paths

def fmt_chain(chain):
    return " -> ".join(packages_by_id[pid]["name"] for pid in chain)

# 3. Zonen-Regeln direkt & transitiv für jede Workspace-Crate auswerten
for root_id in sorted(ws_zones, key=lambda pid: packages_by_id[pid]["name"]):
    root_name = packages_by_id[root_id]["name"]
    root_zone = ws_zones[root_id]
    reachable = bfs_shortest_paths(root_id)

    for target_id, chain in reachable.items():
        if target_id == root_id:
            continue
        target_name = packages_by_id[target_id]["name"]
        target_zone = ws_zones.get(target_id)
        chain_str = fmt_chain(chain)

        if root_zone == "shared":
            if target_zone in ("server", "client"):
                violations.append(
                    f"ZONE VIOLATION: shared crate '{root_name}' depends on {target_zone} crate "
                    f"'{target_name}' via [{chain_str}]"
                )
            if target_name in SHARED_FORBIDDEN_CRATES:
                violations.append(
                    f"ZONE VIOLATION: shared crate '{root_name}' depends on forbidden crate "
                    f"'{target_name}' via [{chain_str}]"
                )

        elif root_zone == "server":
            if target_zone == "client":
                violations.append(
                    f"ZONE VIOLATION: server crate '{root_name}' depends on client crate "
                    f"'{target_name}' via [{chain_str}]"
                )
            if target_name in SERVER_FORBIDDEN_CRATES:
                violations.append(
                    f"ZONE VIOLATION: server crate '{root_name}' depends on forbidden TUI crate "
                    f"'{target_name}' via [{chain_str}]"
                )

        elif root_zone == "client":
            if target_zone == "server":
                violations.append(
                    f"ZONE VIOLATION: client crate '{root_name}' depends on server crate "
                    f"'{target_name}' via [{chain_str}]"
                )
            if target_name in CLIENT_FORBIDDEN_CRATES:
                violations.append(
                    f"ZONE VIOLATION: client crate '{root_name}' depends on forbidden crate "
                    f"'{target_name}' via [{chain_str}]"
                )

if violations:
    print(f"  -> FEHLGESCHLAGEN ({len(violations)} Zonen-Verstöße erkannt):")
    for v in violations:
        print(f"     * {v}")
    sys.exit(1)

print("  -> OK (3-Zonen-Abhängigkeitsgraph vollständig sauber).")
PY
then
    OVERALL_FAILED=1
fi

# ------------------------------------------------------------------------------
# Check 3: Vollständigkeit der Governance-SSoT (.agents/) & Adapter-Dotfiles
# ------------------------------------------------------------------------------
echo ""
echo "[Check 3/4] Prüfe Vollständigkeit der Governance-Dateien & IDE-Adapter..."
REQUIRED_FILES=(
    "AGENTS.md"
    ".agents/00-invariants.md"
    ".agents/01-architecture.md"
    ".agents/02-lore-canon.md"
    ".agents/03-scope-guardrails.md"
    ".agents/04-workflows.md"
    ".agents/05-client-server-boundary.md"
    ".agents/STATUS.md"
    ".agents/decisions/ADR-001-license-and-ircd-isolation.md"
    ".agents/decisions/ADR-002-quic-vs-irc-protocol-split.md"
    ".agents/decisions/ADR-003-pure-deterministic-ash-core.md"
    ".agents/decisions/ADR-004-three-pillar-monorepo-isolation.md"
    ".agents/decisions/ADR-005-ai-verification-harness.md"
    ".agents/tasks/README.md"
    ".agents/tasks/sprint-2-breakdown.md"
    "CLAUDE.md"
    "GEMINI.md"
    ".agent/rules/ash-net.md"
    ".cursor/rules/ash-net.mdc"
    ".github/copilot-instructions.md"
    ".windsurfrules"
    ".aiignore"
    ".cursorignore"
    ".geminiignore"
    "deny.toml"
    "docker-compose.harness.yml"
    "scripts/harness.sh"
    "harness/scenarios/00-smoke-mesh.toml"
    "harness/scenarios/01-sim-invariants.toml"
)

GOV_FAILED=0
for req in "${REQUIRED_FILES[@]}"; do
    if [[ ! -s "$req" ]]; then
        echo "  GOVERNANCE VIOLATION: Pflichtdatei fehlt oder ist leer: '$req'"
        GOV_FAILED=1
    fi
done

if [[ ! -x "scripts/harness.sh" ]]; then
    echo "  GOVERNANCE VIOLATION: 'scripts/harness.sh' ist nicht ausführbar (chmod +x fehlt)"
    GOV_FAILED=1
fi

if [[ "$GOV_FAILED" -eq 0 ]]; then
    echo "  -> OK (${#REQUIRED_FILES[@]} Governance-, Adapter- & Hygiene-Dateien vorhanden)."
else
    OVERALL_FAILED=1
fi

# ------------------------------------------------------------------------------
# Check 4: Rust Quality Gate (fmt, clippy -D warnings, test, optional cargo-deny)
# ------------------------------------------------------------------------------
echo ""
echo "[Check 4/4] Führe Rust Quality Gate aus (fmt, clippy, test, optional cargo-deny)..."

echo "  [4a] cargo fmt --all -- --check"
if cargo fmt --all -- --check --color never -l; then
    echo "       -> fmt OK"
else
    echo "       -> fmt FEHLGESCHLAGEN (Formatierungsabweichungen vorhanden)"
    OVERALL_FAILED=1
fi

echo "  [4b] cargo clippy --workspace --all-targets -- -D warnings"
if cargo clippy --workspace --all-targets -- -D warnings; then
    echo "       -> clippy OK"
else
    echo "       -> clippy FEHLGESCHLAGEN"
    OVERALL_FAILED=1
fi

echo "  [4c] cargo test --workspace"
if cargo test --workspace; then
    echo "       -> tests OK"
else
    echo "       -> tests FEHLGESCHLAGEN"
    OVERALL_FAILED=1
fi

if command -v cargo-deny >/dev/null 2>&1; then
    echo "  [4d] cargo deny check licenses"
    if cargo deny check licenses; then
        echo "       -> cargo-deny OK"
    else
        echo "       -> cargo-deny FEHLGESCHLAGEN"
        OVERALL_FAILED=1
    fi
else
    echo "  [4d] cargo-deny nicht installiert (übersprungen; Konfiguration liegt in deny.toml bereit)."
fi

echo ""
echo "========================================================================"
if [[ "$OVERALL_FAILED" -ne 0 ]]; then
    echo " ERGEBNIS: GATE NICHT GRÜN — Bitte gemeldete Verstöße beheben!"
    echo "========================================================================"
    exit 1
fi

echo " ERGEBNIS: ALLE INVARIANT- & ARCHITEKTUR-CHECKS GRÜN (EXIT 0)"
echo "========================================================================"
