#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
source=$(cd "${NANOCODE_TS:-../nanocode-ts}" && pwd)
ref=${NANOCODE_TS_REF:-origin/master}
modified=""
if [ "$ref" = worktree ]; then
  branch=$(git -C "$source" rev-parse --abbrev-ref HEAD)
  revision=$(git -C "$source" rev-parse HEAD)
  snapshot=$(git -C "$source" stash create)
  if [ -n "$snapshot" ]; then modified=$',\n  "modified": true'; fi
else
  branch=${ref#origin/}
  revision=$(git -C "$source" rev-parse "$ref")
  snapshot=""
fi
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git -C "$source" archive "${snapshot:-$revision}" src/bash src/internal src/Contracts.ts src/wasm/prelude.js src/wasm/prelude | tar -x -C "$work"
printf "export { create } from './src/bash/Bash.ts'\nexport { FsError } from './src/bash/FileSystem.ts'\nexport { pip, sitePackages } from './src/bash/internal/commands/pip.ts'\n" > "$work/entry.ts"
mkdir -p Sources/Nanocode/Resources/shell/prelude
(cd "$work" && "$source/node_modules/.bin/rolldown" entry.ts --format iife --name NanocodeBash --file bash.js \
  --no-codeSplitting --platform neutral --logLevel silent >/dev/null)
cp "$work/bash.js" Sources/Nanocode/Resources/shell/bash.js
cp "$work/src/wasm/prelude.js" Sources/Nanocode/Resources/shell/prelude.js
cp "$work"/src/wasm/prelude/*.js Sources/Nanocode/Resources/shell/prelude/
printf '{\n  "repository": "https://github.com/nanocodexyz/nanocode-ts",\n  "branch": "%s",\n  "revision": "%s"%s\n}\n' "$branch" "$revision" "$modified" > ts-revision.json
