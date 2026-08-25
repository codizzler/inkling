#!/usr/bin/env sh
# One-time bootstrap: publish the seven per-platform npm packages by hand.
#
#   sh scripts/stage-platform-packages.sh      # assemble from a CI build
#   npm login                                  # once, if not already logged in
#   sh scripts/publish-platform-packages.sh    # prompts for an OTP per package
#
# Why this is manual. The Node addon ships one npm package per platform, and the
# main package depends on all of them optionally. npm trusted publishing cannot
# create a package that does not exist yet, there being no pending-publisher
# flow as there is on PyPI, and granular tokens with Bypass 2FA are now
# restricted for direct publishing, so CI cannot create these names either.
#
# Once the names exist this is never needed again: CI only ever publishes new
# *versions* of packages that are already there, which an ordinary token can do,
# and a trusted publisher can then be configured per package to drop tokens
# entirely.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root/crates/inkling-node"

[ -d npm ] || {
    echo "npm/ is not staged; run scripts/stage-platform-packages.sh first" >&2
    exit 1
}

published=0
skipped=0
outstanding=0

for dir in npm/*/; do
    name=$(sed -n 's/.*"name": "\(.*\)",/\1/p' "$dir/package.json" | head -n 1)
    version=$(sed -n 's/.*"version": "\(.*\)",/\1/p' "$dir/package.json" | head -n 1)

    if ! ls "$dir"*.node >/dev/null 2>&1; then
        echo "SKIP  $name has no binary staged" >&2
        outstanding=$((outstanding + 1))
        continue
    fi
    if npm view "${name}@${version}" version >/dev/null 2>&1; then
        echo "ok    ${name}@${version} is already published"
        skipped=$((skipped + 1))
        continue
    fi

    printf '\n%s@%s\n' "$name" "$version"
    printf 'one-time password (blank to skip): '
    read -r otp
    if [ -z "$otp" ]; then
        echo "skipped"
        outstanding=$((outstanding + 1))
        continue
    fi

    if (cd "$dir" && npm publish --access public --otp "$otp"); then
        echo "ok    published ${name}@${version}"
        published=$((published + 1))
    else
        echo "FAIL  ${name}@${version}" >&2
        outstanding=$((outstanding + 1))
    fi
done

printf '\n%s published, %s already there, %s still outstanding\n' \
    "$published" "$skipped" "$outstanding"
echo "re-running is safe: anything already on the registry is skipped"
[ "$outstanding" -eq 0 ] || exit 1
