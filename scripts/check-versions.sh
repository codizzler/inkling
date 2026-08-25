#!/usr/bin/env sh
# Assert that every manifest in the repo carries the same version, and that it
# matches the release tag when one is given.
#
#   scripts/check-versions.sh          # all manifests agree
#   scripts/check-versions.sh v0.2.0   # ...and agree with the tag
#
# The version is hand-maintained across five Cargo.tomls and a package.json.
# Tagging v0.2.0 against 0.1.5 manifests silently republishes 0.1.5, so this
# runs in CI before anything is pushed to a registry.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

# First `version = "..."` in a Cargo.toml is the [package] one.
pkg_version() {
    sed -n 's/^version = "\(.*\)"/\1/p' "$1" | head -n 1
}

want=$(pkg_version crates/inkling/Cargo.toml)
[ -n "$want" ] || { echo "could not read a version from crates/inkling/Cargo.toml" >&2; exit 1; }

status=0
report() {
    if [ "$2" = "$want" ]; then
        printf '  ok    %-40s %s\n' "$1" "$2"
    else
        printf '  WRONG %-40s %s (expected %s)\n' "$1" "${2:-<none>}" "$want"
        status=1
    fi
}

for manifest in \
    crates/inkling/Cargo.toml \
    crates/inkling-cli/Cargo.toml \
    crates/inkling-py/Cargo.toml \
    crates/inkling-wasm/Cargo.toml \
    crates/inkling-node/Cargo.toml
do
    report "$manifest" "$(pkg_version "$manifest")"
done

# The CLI depends on the library by registry version; a stale pin here publishes
# a binary built against the previous release.
report "inkling-cli -> inkling-loader dep" \
    "$(sed -n 's/.*package = "inkling-loader".*version = "\([^"]*\)".*/\1/p' crates/inkling-cli/Cargo.toml | head -n 1)"

report "crates/inkling-node/package.json" \
    "$(sed -n 's/^  "version": "\(.*\)",$/\1/p' crates/inkling-node/package.json | head -n 1)"

if [ "$#" -ge 1 ]; then
    tag=${1#v}
    report "git tag" "$tag"
fi

if [ "$status" -eq 0 ]; then
    echo "all manifests at $want"
else
    echo "version mismatch: fix the manifests above before releasing" >&2
fi
exit "$status"
