#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

required_paths=(
  "$repo_root/docs/examples/scheduler/pharmacy-course-demo.example.jsonc"
  "$repo_root/docs/examples/scheduler/pharmacy-evidence-team.example.jsonc"
  "$repo_root/docs/examples/scheduler/pharmacy-prometheus.example.jsonc"
  "$repo_root/docs/examples/scheduler/pharmacy-atlas.example.jsonc"
  "$repo_root/docs/examples/scheduler/pharmacy-autoresearch-course.runnable.example.jsonc"
  "$repo_root/docs/examples/scheduler/pharmacy-autoresearch-research.runnable.example.jsonc"
  "$repo_root/scripts/evaluate-course-qa.sh"
  "$repo_root/scripts/evaluate-natural-product-model.sh"
)

for path in "${required_paths[@]}"; do
  if [[ ! -f "$path" ]]; then
    echo "missing required demo file: $path" >&2
    exit 1
  fi
done

python3 -m py_compile "$repo_root/scripts/autoresearch_e2e_smoke.py" >/dev/null

echo "score=1"
