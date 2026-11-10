#!/usr/bin/env bash
# Run the three e2e legs behind one slot lease (the build-slot e2e wrapper
# rebuilds once, this script launches the game three times):
#
#   e2e-slice1.log    EMBEDDED=0  the content as a dev mod           must PASS, exit 0
#   e2e-embedded.log  EMBEDDED=1  only the .hnmod, plus CONTROL=1     must PASS, exit 0
#   e2e-selftest.log  SELFTEST=1  the conservation assertion broken   must FAIL, non-zero
#
# Exits 0 only when both positive legs pass and the selftest really fails.
set -uo pipefail
PORT=$(cd "$(dirname "$0")/.." && pwd)
EVIDENCE=$PORT/../_shared/evidence/naturesaura
mkdir -p "$EVIDENCE"

run() { # run NAME ENV… — sets RC
    local name=$1; shift
    echo "== $name ==" >&2
    env "$@" "$PORT/tools/e2e.sh" > "$EVIDENCE/$name.log" 2>&1
    RC=$?
    echo "  $name exit $RC ($(grep -c . "$EVIDENCE/$name.log") log lines)" >&2
}

run e2e-slice1 EMBEDDED=0
PASS_DEV=$RC
run e2e-embedded EMBEDDED=1 CONTROL=1
PASS_EMBEDDED=$RC
run e2e-selftest SELFTEST=1
SELFTEST=$RC

echo "== verdict =="
echo "  e2e-slice1   exit $PASS_DEV     (want 0)"
echo "  e2e-embedded exit $PASS_EMBEDDED     (want 0)"
echo "  e2e-selftest exit $SELFTEST     (want non-zero)"
if [ "$PASS_DEV" -eq 0 ] && [ "$PASS_EMBEDDED" -eq 0 ] && [ "$SELFTEST" -ne 0 ]; then
    echo "PASS: the loop works with the content as a dev mod and inside one .hnmod, and the selftest leg really fails"
    exit 0
fi
echo "FAIL: a leg answered differently (slice1=$PASS_DEV embedded=$PASS_EMBEDDED selftest=$SELFTEST)"
exit 1
