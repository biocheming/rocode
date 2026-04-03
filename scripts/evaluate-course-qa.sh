#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
asset_dir="$repo_root/docs/examples/scheduler/demo_assets/course_qa"

knowledge_base="$asset_dir/knowledge_base/natural_products_notes.md"
prompt_file="$asset_dir/prompts/system_prompt.txt"
question_file="$asset_dir/eval/course_questions.tsv"

for path in "$knowledge_base" "$prompt_file" "$question_file"; do
  if [[ ! -f "$path" ]]; then
    echo "missing demo asset: $path" >&2
    exit 1
  fi
done

score="$(
  python3 - "$knowledge_base" "$prompt_file" "$question_file" <<'PY'
import pathlib
import sys

knowledge = pathlib.Path(sys.argv[1]).read_text()
prompt = pathlib.Path(sys.argv[2]).read_text()
questions = pathlib.Path(sys.argv[3]).read_text().splitlines()

score = 50
score += knowledge.count("## ")
score += knowledge.count("- ")
score += prompt.lower().count("teaching")
score += prompt.lower().count("pharmacy")
score += sum(1 for line in questions if line.strip() and not line.startswith("#"))

print(score)
PY
)"

echo "score=${score}"
