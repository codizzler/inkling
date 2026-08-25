#!/usr/bin/env sh
# Assemble the per-platform npm packages locally from a CI build, so they can be
# published by hand. Mirrors exactly what .github/workflows/npm.yml does.
#
#   sh scripts/stage-platform-packages.sh [run-id]
#
# With no run id it uses the most recent successful npm workflow run. The
# binaries are whatever CI built, and nothing is compiled here, so this works
# from any machine regardless of what it can cross compile.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root/crates/inkling-node"

run=${1:-}
if [ -z "$run" ]; then
    run=$(gh run list --workflow npm.yml --status success --limit 1 --json databaseId -q '.[0].databaseId')
    echo "using the last successful npm run: $run"
fi

rm -rf artifacts npm
gh run download "$run" -D artifacts
npm install --silent
npx napi create-npm-dir --target .
npx napi artifacts

echo
for dir in npm/*/; do
    name=$(sed -n 's/.*"name": "\(.*\)",/\1/p' "$dir/package.json" | head -n 1)
    if ls "$dir"*.node >/dev/null 2>&1; then
        printf 'staged  %s\n' "$name"
    else
        printf 'MISSING %s has no binary\n' "$name" >&2
    fi
done
