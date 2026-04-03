#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
asset_dir="$repo_root/docs/examples/scheduler/demo_assets/research_analysis"

taxonomy_file="$asset_dir/data/taxonomy.tsv"
cases_file="$asset_dir/eval/cases.tsv"
analysis_file="$asset_dir/analysis/notes.md"

for path in "$taxonomy_file" "$cases_file" "$analysis_file"; do
  if [[ ! -f "$path" ]]; then
    echo "missing demo asset: $path" >&2
    exit 1
  fi
done

score="$(
  python3 - "$taxonomy_file" "$cases_file" "$analysis_file" <<'PY'
import pathlib
import sys

taxonomy = pathlib.Path(sys.argv[1]).read_text().splitlines()
cases = pathlib.Path(sys.argv[2]).read_text().splitlines()
analysis = pathlib.Path(sys.argv[3]).read_text()

score = 60
score += sum(1 for line in taxonomy if line.strip() and not line.startswith("#"))
score += sum(1 for line in cases if line.strip() and not line.startswith("#"))
score += analysis.lower().count("evidence")
score += analysis.lower().count("class")

print(score)
PY
)"

echo "score=${score}"
