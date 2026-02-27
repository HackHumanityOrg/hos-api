#!/usr/bin/env bash
set -euo pipefail

THRESHOLD="${1:-85}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_JSON="$(mktemp)"
trap 'rm -f "$TMP_JSON"' EXIT

cd "$ROOT_DIR"

cargo llvm-cov \
  -p discourse-indexer \
  --all-features \
  --json \
  --summary-only \
  --output-path "$TMP_JSON" \
  -- \
  --test-threads=1

python3 - "$TMP_JSON" "$THRESHOLD" <<'PY'
import json
import sys

path = sys.argv[1]
threshold = float(sys.argv[2])

with open(path, "r", encoding="utf-8") as f:
    data = json.load(f)

records = data.get("data", [])
if not records:
    print("No coverage records found in llvm-cov JSON output", file=sys.stderr)
    sys.exit(2)

files = records[0].get("files", [])
target_prefix = "indexers/discourse-indexer/src/"

total = 0
covered = 0

for file_entry in files:
    filename = file_entry.get("filename", "").replace("\\", "/")
    if target_prefix not in filename:
        continue

    if filename.endswith("indexers/discourse-indexer/src/main.rs"):
        continue

    lines = file_entry.get("summary", {}).get("lines", {})
    total += int(lines.get("count", 0))
    covered += int(lines.get("covered", 0))

if total == 0:
    print("No discourse-indexer src files were included in coverage aggregation", file=sys.stderr)
    sys.exit(2)

percent = (covered / total) * 100
print(f"discourse-indexer (src/, excluding main.rs) line coverage: {percent:.2f}% ({covered}/{total})")

if percent < threshold:
    print(
        f"Coverage gate failed: {percent:.2f}% < required {threshold:.2f}%",
        file=sys.stderr,
    )
    sys.exit(1)

print(f"Coverage gate passed: {percent:.2f}% >= required {threshold:.2f}%")
PY
