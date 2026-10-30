#!/usr/bin/env bash
# Bump workspace version in Cargo.toml, commit, and create annotated git tag v<Version>
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: ./scripts/release.sh <version> [--push]" >&2
  exit 1
fi

VERSION="${1#v}"
PUSH="${2:-}"

sed -i.bak -E "s/^version = \"[^\"]+\"/version = \"${VERSION}\"/" Cargo.toml
rm -f Cargo.toml.bak

git add Cargo.toml
git commit -m "chore(release): v${VERSION}"
git tag -a "v${VERSION}" -m "Serika Code v${VERSION}"

if [[ "$PUSH" == "--push" ]]; then
  git push origin main --follow-tags
fi
