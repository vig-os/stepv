#!/usr/bin/env bash
# The vendored three.js for spike B (#27): fetched, pinned by sha256, not
# committed (minified JS fights the repo's typo and whitespace hooks; that is
# itself one of the spike's findings).
set -euo pipefail
cd "$(dirname "$0")/static"
v=0.186.1
fetch() { # url, file
  curl -sfL -o "$2" "$1"
}
fetch "https://cdn.jsdelivr.net/npm/three@$v/build/three.module.min.js" three.module.min.js
fetch "https://cdn.jsdelivr.net/npm/three@$v/build/three.core.min.js" three.core.min.js
fetch "https://cdn.jsdelivr.net/npm/three@$v/examples/jsm/controls/OrbitControls.js" OrbitControls.js
shasum -a 256 -c <<'SUMS'
3bc833fceb6577bd1a380388f832ae61cd6e2f78ad02b0bf0d5adf5a4a9334fe  three.module.min.js
3b346151f65ffdfca3e4c002bd58966b78c423087fb48a873f83200de1bffc48  three.core.min.js
3d79d07ecb686b4e5d93232eedab255331c1beef711e13164eaa1f68655a5f2b  OrbitControls.js
SUMS
