#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT_CARGO="$ROOT_DIR/Cargo.toml"
ROOT_LOCK="$ROOT_DIR/Cargo.lock"
PLUGIN_LOCK="$ROOT_DIR/docs/examples/plugins_example/rust/Cargo.lock"

VERSION="$(
  perl -0ne '
    if (/\[workspace\.package\]\n(?:.*\n)*?version = "(\d{4}\.\d{1,2}\.\d{1,2})"/s) {
      print "$1\n";
      exit 0;
    }
    exit 1;
  ' "$ROOT_CARGO"
)"

VERSION_TAG="v$VERSION"
IFS='.' read -r YEAR MONTH DAY <<< "$VERSION"
DATE_ISO="$(printf "%04d-%02d-%02d" "$YEAR" "$MONTH" "$DAY")"

replace_in_file() {
  local file="$1"
  perl -0pi -e '
    s/v\d{4}\.\d{1,2}\.\d{1,2}/$ENV{VERSION_TAG}/g;
    s/\d{4}-\d{2}-\d{2}/$ENV{DATE_ISO}/g;
    s/\d{4}\.\d{1,2}\.\d{1,2}/$ENV{VERSION}/g if $ARGV =~ /Cargo\.lock$/;
    s/\d{4}\.\d{1,2}\.\d{1,2}/$ENV{VERSION}/g
      if $ARGV =~ /(docs\/index\.md|docs\/installation\.md|package(?:-lock)?\.json)$/;
  ' "$file"
}

VERSION="$VERSION" VERSION_TAG="$VERSION_TAG" DATE_ISO="$DATE_ISO" replace_in_file "$ROOT_LOCK"

if [[ -f "$PLUGIN_LOCK" ]]; then
  VERSION="$VERSION" VERSION_TAG="$VERSION_TAG" DATE_ISO="$DATE_ISO" replace_in_file "$PLUGIN_LOCK"
fi

for file in \
  "$ROOT_DIR/README.md" \
  "$ROOT_DIR/USER_GUIDE.md" \
  "$ROOT_DIR/docs/README.md" \
  "$ROOT_DIR/docs/index.md" \
  "$ROOT_DIR/docs/installation.md" \
  "$ROOT_DIR/docs/examples/plugins_example/README.md" \
  "$ROOT_DIR/docs/examples/plugins_example/skill/SKILL.md" \
  "$ROOT_DIR/crates/rocode-server/web/package.json" \
  "$ROOT_DIR/crates/rocode-server/web/package-lock.json"
do
  VERSION="$VERSION" VERSION_TAG="$VERSION_TAG" DATE_ISO="$DATE_ISO" replace_in_file "$file"
done

echo "Synced version $VERSION_TAG ($DATE_ISO)"
