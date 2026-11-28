#!/usr/bin/env bash
# Publishes the packages that npm/assemble.mjs built, platform packages first
# so the launcher's optional dependencies exist before anyone installs it.
# Versions already on npm are skipped, so a failed run can be re-run.
#
#   bash npm/publish.sh <version> [out-dir]
#
# Authentication:
# - Normally npm trusted publishing (OIDC): no secret at all. The job needs
#   `id-token: write`, npm >= 11.5.1, and a trusted publisher configured for
#   each package on npmjs.com (see docs/RELEASING.md).
# - Only when NODE_AUTH_TOKEN is set (the one-time first publish, before the
#   packages exist and trusted publishing can be configured): that token.
# Both add npm provenance, a signed link from each package to this run.
set -euo pipefail

version="${1:?usage: npm/publish.sh <version> [out-dir]}"
out="${2:-npm-out}"

if [ -n "${NODE_AUTH_TOKEN:-}" ]; then
  echo "Authenticating with the NPM_TOKEN secret"
  npmrc="${RUNNER_TEMP:?}/.npmrc"
  # npm, not the shell, expands the variable; the token is never written out.
  # shellcheck disable=SC2016
  printf '//registry.npmjs.org/:_authToken=${NODE_AUTH_TOKEN}\n' > "$npmrc"
  export NPM_CONFIG_USERCONFIG="$npmrc"
else
  echo "Authenticating with npm trusted publishing (OIDC)"
fi

for dir in "$out"/platforms/* "$out"/prunecss; do
  # An absolute path, so npm reads it as a folder and never as a package
  # name or a GitHub "owner/repo" shorthand.
  dir="$(cd "$dir" && pwd)"
  name="$(node -p 'require(process.argv[1] + "/package.json").name' "$dir")"
  if npm view "$name@$version" version > /dev/null 2>&1; then
    echo "$name@$version is already on npm; skipping"
  else
    npm publish "$dir" --access public --provenance
  fi
done
