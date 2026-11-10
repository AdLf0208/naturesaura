#!/usr/bin/env bash
# Package both artifacts:
#  - the .hmod via the pin's own cargo-horizon-mod (build + check), from content/
#  - the source-only .hnmod (tools/package.py, deterministic bytes, in the port root),
#    carrying the built .hmod at `mixins/naturesaura/data/naturesaura-content.hmod`
#    (the AE2 pattern: one installed file gives the blocks and the code).
# Run inside a build slot (it compiles the packaging tool):
#   ports/tools/build-slot.sh run --job na-pack --port ports/naturesaura \
#       --step package -- env CARGO_INCREMENTAL=0 tools/package.sh
set -euo pipefail

PORT=$(cd "$(dirname "$0")/.." && pwd)
HORIZON=${HORIZON:-$PORT/../_shared/pins/horizon-1d63e0a}
LOGS=$PORT/target/ladder
mkdir -p "$LOGS"
cd "$PORT"

CARGO_TARGET_DIR=$PORT/target/carry-tools
export CARGO_TARGET_DIR
(cd content && cargo run --locked --offline --quiet \
    --manifest-path "$HORIZON/crates/cargo-horizon-mod/Cargo.toml" -- horizon-mod build) \
    2>&1 | tee "$LOGS/package-hmod.log"
cp content/target/horizon-mod/naturesaura.hmod content/naturesaura.hmod
# The bundle carries the very file it was built with; package.py refuses a stale copy.
mkdir -p mixins/naturesaura/data
cp content/naturesaura.hmod mixins/naturesaura/data/naturesaura-content.hmod

python3 tools/package.py | tee "$LOGS/package-hnmod.log"
cp target/naturesaura-0.1.0.hnmod naturesaura.hnmod

(cd content && cargo run --locked --offline --quiet \
    --manifest-path "$HORIZON/crates/cargo-horizon-mod/Cargo.toml" -- horizon-mod check naturesaura.hmod) \
    2>&1 | tee -a "$LOGS/package-hmod.log"

echo "== artifacts =="
for f in content/naturesaura.hmod naturesaura.hnmod; do
    [ -f "$f" ] || { echo "missing $f"; exit 1; }
    size=$(stat -c %s "$f")
    sha=$(sha256sum "$f" | cut -d' ' -f1)
    entries=$(python3 -c 'import zipfile,sys; print(len(zipfile.ZipFile(sys.argv[1]).namelist()))' "$f")
    echo "$f size=$size sha256=$sha entries=$entries"
done
