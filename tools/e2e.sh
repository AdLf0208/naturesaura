#!/usr/bin/env bash
# In-game check for Nature's Aura slice 1: an aura bloom stores 150,000 aura in
# its chunk's drain spots, and a nature's altar inside its multiblock pulls that
# aura into its own container and turns an iron ingot into infused iron for
# exactly the recipe's cost — while an altar outside a multiblock and an altar in
# an area with no aura do nothing.
#
# Every mod command is typed with `ctl chat "/cmd"` because `ctl command` runs
# the console path and bypasses the native `perform_command` hook.
#
# The Overworld, high above the terrain at x=200, z=0: a dirt floor at y=199 with
# clear air above it. The altar stands at (200, 200, 0), which is where the
# multiblock's pattern puts it; the bloom is planted at (206, 200, -6), six
# blocks away and inside the altar's 20-block aura radius. The aura type is the
# Overworld's, so the altar pulls aura (upstream pulls only in the Overworld and
# the Nether).
#
# Default run (EMBEDDED=0): the game loads `content/` as a dev mod.
# EMBEDDED=1: the game is launched with **only the native bundle** (no
# `--dev-mod`) and the content comes from the bundle's embedded copy
# (`mixins/<id>/data/…`, the AE2 pattern). The run also checks the file the
# helper wrote, and CONTROL=1 (default with EMBEDDED) first launches the same
# binary with `.horizon-cache` unusable, where the ids must be unknown.
#
# SELFTEST=1 breaks the conservation assertion on purpose (it must FAIL), proving
# the scenario can fail.
set -Eeuo pipefail
export LC_ALL=C
[ -z "${NA_DEBUG:-}" ] || set -x

EMBEDDED=${EMBEDDED:-0}
CONTROL=${CONTROL:-$EMBEDDED}
SELFTEST=${SELFTEST:-0}
KEEP=${KEEP:-0}

CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PORT=$CLONE/ports/naturesaura
PIN_BIN=$CLONE/ports/_shared/bin/horizon-1d63e0a5
CTL=${CTL:-$PIN_BIN/horizon-ctl}
GAME=${GAME:-$PORT/target/horizon-cargo/fast/horizon-client}
RES=${RES:-${HORIZON_RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}}
SOCK=${SOCK:-/tmp/opencode/naturesaura-e2e.sock}
WORLD=NatureAuraE2E
CONTROL_WORLD=NatureAuraControl

# The altar's cell and the arena around it.
AX=200
AY=200
AZ=0
# The bloom's cell (dirt below it, inside the altar's radius).
BX=206
BY=200
BZ=-6
# An altar with no multiblock around it, still inside the bloom's radius.
CX=210
CY=200
CZ=6
# The chunk base every area read starts from, and what the bloom adds.
DEFAULT_AURA=1000000
BLOOM_AURA=150000
# `floor(axe)`: ceil(7500/40) = ceil(15000/80) = 188 aura a tick, 80 ticks.
RECIPE_TICKS=80
AURA_PER_TICK=188
RECIPE_SPEND=$((AURA_PER_TICK * RECIPE_TICKS))
TOTAL_AURA=$((DEFAULT_AURA + BLOOM_AURA))
AREA_AFTER=$((TOTAL_AURA - RECIPE_SPEND))

[ -x "$CTL" ] || { echo "missing horizon-ctl: $CTL" >&2; exit 1; }
[ -x "$GAME" ] || { echo "missing horizon-client: $GAME" >&2; exit 1; }
[ -d "$RES/assets" ] || { echo "missing prepared resources: $RES" >&2; exit 1; }

SCRATCH=$(mktemp -d /tmp/opencode/naturesaura-home.XXXXXX)
export HORIZON_HOME=$SCRATCH/game
export HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data
export XDG_CONFIG_HOME=$SCRATCH/config
export HORIZON_NO_AUDIO=1
mkdir -p "$(dirname "$SOCK")" "$XDG_CONFIG_HOME"
rm -f "$SOCK"

ctl() { "$CTL" --socket "$SOCK" "$@"; }
say() { ctl chat "/$1" >/dev/null; }
fail() { echo "FAIL: $*" >&2; ctl logs --lines 80 >&2 || true; exit 1; }
on_error() { echo "FAIL: line $1: '$2' exited $?" >&2; ctl logs --lines 40 >&2 || true; }
trap 'on_error "$LINENO" "$BASH_COMMAND"' ERR

cleanup() {
    local status=$?
    ctl quit >/dev/null 2>&1 || true
    sleep 1
    if [ "$KEEP" = 1 ]; then
        echo "kept scratch: $SCRATCH" >&2
    else
        rm -rf "$SCRATCH" 2>/dev/null || true
    fi
    # Last line of every run: `check-e2e-logs.py` reads it as the verdict.
    echo "EXIT:$status"
}
trap cleanup EXIT

launch() { # launch [WORLD]
    local world=${1:-$WORLD}
    rm -f "$SOCK"
    cd "$SCRATCH"
    local args=(--game "$GAME" --world "$world" --seed 20261105 --creative --headless
                --wait --resources "$RES")
    if [ "$EMBEDDED" = 1 ]; then
        echo "  launch: no --dev-mod; the bundle carries its own content" >&2
    else
        args+=(--dev-mod "$PORT/content")
    fi
    "$CTL" --socket "$SOCK" launch "${args[@]}" >/dev/null
    ctl wait-ready 120
    ctl wait 40
}

block_name() { ctl block "$1" "$2" "$3" | jq -r '.name // .block // empty'; }

# The arena: the player first (so the chunks load), then the floor, then clear air,
# then no ambient mobs. Standing on the floor keeps the player from falling.
arena() {
    ctl command "execute in minecraft:overworld run tp @s $AX.5 $AY $AZ.5" >/dev/null
    local loaded=false
    for _ in $(seq 1 120); do
        ctl wait 5 >/dev/null
        if [ "$(ctl block "$AX" $((AY - 1)) "$AZ" | jq -r '.loaded // false')" = true ]; then
            loaded=true
            break
        fi
    done
    [ "$loaded" = true ] || fail "the arena chunk never loaded"
    ctl command "execute in minecraft:overworld run fill $((AX - 10)) $((AY - 1)) $((AZ - 10)) $((AX + 12)) $((AY - 1)) $((AZ + 10)) minecraft:dirt" >/dev/null
    ctl wait 6 >/dev/null
    ctl command "execute in minecraft:overworld run fill $((AX - 10)) $AY $((AZ - 10)) $((AX + 12)) $((AY + 8)) $((AZ + 10)) minecraft:air" >/dev/null
    ctl wait 6 >/dev/null
    ctl command "execute in minecraft:overworld run kill @e[type=!player]" >/dev/null
    ctl command "execute in minecraft:overworld run tp @s $AX.5 $AY $AZ.5" >/dev/null
    ctl wait 20
    [ "$(block_name "$AX" $((AY - 1)) "$AZ")" = "minecraft:dirt" ] || fail "the arena floor is not dirt"
    [ "$(block_name "$AX" "$AY" "$AZ")" = "minecraft:air" ] || fail "the arena air above the floor is not air"
}

# `/naturesaura aura X Y Z R`: the line, then AREA, BASE, SPOTS, COUNT.
aura() { # aura X Y Z [R]
    local radius=${4:-20}
    say "naturesaura aura $1 $2 $3 $radius"
    ctl wait 8 >/dev/null
    local line
    line=$(ctl messages 12 | jq -r '.chat[]' | grep -m1 "Nature's Aura at" || true)
    [ -n "$line" ] || fail "no /naturesaura aura reply"
    AREA=$(sed -n 's/.* area=\(-\{0,1\}[0-9]\{1,\}\).*/\1/p' <<<"$line")
    BASE=$(sed -n 's/.* base=\(-\{0,1\}[0-9]\{1,\}\).*/\1/p' <<<"$line")
    SPOTS=$(sed -n 's/.* spots=\(-\{0,1\}[0-9]\{1,\}\).*/\1/p' <<<"$line")
    COUNT=$(sed -n 's/.* count=\([0-9]\{1,\}\).*/\1/p' <<<"$line")
    [ -n "$AREA" ] && [ -n "$SPOTS" ] && [ -n "$COUNT" ] || fail "the aura line did not parse: $line"
    echo "  $line" >&2
}

# `/naturesaura spot X Y Z`: SPOT.
spot() { # spot X Y Z
    say "naturesaura spot $1 $2 $3"
    ctl wait 8 >/dev/null
    local line
    line=$(ctl messages 12 | jq -r '.chat[]' | grep -m1 'Aura spot at' || true)
    [ -n "$line" ] || fail "no /naturesaura spot reply"
    SPOT=$(sed -n 's/.*: \(-\{0,1\}[0-9]\{1,\}\)$/\1/p' <<<"$line")
    [ -n "$SPOT" ] || fail "the spot line did not parse: $line"
    echo "  $line" >&2
}

# `/naturesaura altar X Y Z`: COMPLETE, ALTAR_AURA, TIMER, RECIPE, SLOT, AREA.
altar() { # altar X Y Z
    say "naturesaura altar $1 $2 $3"
    ctl wait 8 >/dev/null
    local line
    line=$(ctl messages 12 | jq -r '.chat[]' | grep -m1 "Nature's Altar at" || true)
    [ -n "$line" ] || fail "no /naturesaura altar reply"
    COMPLETE=$(sed -n 's/.* complete=\([0-9]\{1,\}\).*/\1/p' <<<"$line")
    ALTAR_AURA=$(sed -n 's/.* aura=\([0-9]\{1,\}\)\/[0-9]\{1,\}.*/\1/p' <<<"$line")
    TIMER=$(sed -n 's/.* timer=\([0-9]\{1,\}\).*/\1/p' <<<"$line")
    RECIPE=$(sed -n 's/.* recipe=\([^ ]*\).*/\1/p' <<<"$line")
    SLOT=$(sed -n 's/.* slot=\(.*\) area=.*/\1/p' <<<"$line")
    AREA=$(sed -n 's/.* area=\(-\{0,1\}[0-9]\{1,\}\).*/\1/p' <<<"$line")
    [ -n "$COMPLETE" ] && [ -n "$ALTAR_AURA" ] && [ -n "$SLOT" ] && [ -n "$AREA" ] \
        || fail "the altar line did not parse: $line"
    echo "  $line" >&2
}

# Hold `count` of an item in hotbar slot 1 and select it.
hold() { # hold ITEM COUNT
    ctl command "item replace entity @s hotbar.0 with $1 $2" >/dev/null
    ctl wait 4 >/dev/null
    ctl select 1 >/dev/null
    ctl wait 2 >/dev/null
}

# Right-click a block with what is held (or with an empty hand). The click aims
# at the block itself: the altar is 12/16 high, so aiming at its "up" face sends
# the ray over it, and `ctl state`'s `target` stays null until the click's own
# aim runs, so the click is what proves the aim.
click() { # click X Y Z
    local x=$1 y=$2 z=$3
    ctl command "execute in minecraft:overworld run tp @s $((x - 2)).5 $y $z.5" >/dev/null
    ctl wait 6 >/dev/null
    ctl look-at-block "$x" "$y" "$z" >/dev/null
    ctl wait 3 >/dev/null
    ctl interact "$x" "$y" "$z" >/dev/null || fail "ctl interact at $x,$y,$z failed"
    ctl wait 6 >/dev/null
}

expect() { # expect NAME GOT WANT
    [ "$2" = "$3" ] || fail "$1 is $2, expected $3"
}

# The control for EMBEDDED=1: the same binary with the embedded content made
# unavailable — `.horizon-cache` is a plain file, so the helper can neither
# compare nor write its content file and falls back to `Continue`. Every id must
# then be unknown; if they resolve, the run proves nothing about the embedded file.
control_leg() {
    echo "== control: the same game with no embedded content file =="
    rm -rf "$SCRATCH/.horizon-cache"
    : > "$SCRATCH/.horizon-cache"
    launch "$CONTROL_WORLD"
    arena
    ctl command "item replace entity @s hotbar.0 with naturesaura:aura_bloom 1" >/dev/null || true
    ctl wait 4 >/dev/null
    ctl inventory | jq -e '[.hotbar[], .main[] | select(.item == "naturesaura:aura_bloom")] | length == 0' >/dev/null \
        || fail "the control game has the bloom item, so the content did not come from the embedded file"
    ctl command "execute in minecraft:overworld run setblock $BX $BY $BZ naturesaura:aura_bloom" >/dev/null || true
    ctl wait 4 >/dev/null
    local name
    name=$(block_name "$BX" "$BY" "$BZ")
    [ "$name" = "minecraft:air" ] || fail "the control game holds $name, so the block id was known"
    [ -f "$SCRATCH/.horizon-cache" ] || fail "the control's .horizon-cache was replaced: the helper wrote its content"
    echo "  without the embedded file: no bloom item, the block stayed $name"
    ctl quit >/dev/null 2>&1 || true
    rm -f "$SCRATCH/.horizon-cache"
}

# EMBEDDED=1: the bundle must have written its content out at startup, byte for
# byte the delivered content mod.
embedded_content_check() {
    local cache=$SCRATCH/.horizon-cache/naturesaura-embedded-content.hmod
    [ -f "$cache" ] || fail "the bundle did not write its embedded content to $cache"
    cmp -s "$cache" "$PORT/content/naturesaura.hmod" \
        || fail "the embedded content differs from content/naturesaura.hmod"
    echo "  embedded content: $(wc -c < "$cache" | tr -d ' ') B at $cache, identical to content/naturesaura.hmod"
}

echo "== launch (Overworld arena, content + native bundle) =="
if [ "$CONTROL" = 1 ]; then control_leg; fi
launch
if [ "$EMBEDDED" = 1 ]; then embedded_content_check; fi
arena

echo "== A: an untouched chunk reads the base aura =="
aura "$AX" "$AY" "$AZ" 20
expect "the area's base aura" "$BASE" "$DEFAULT_AURA"
expect "the area's spot sum" "$SPOTS" 0
expect "the area's spot count" "$COUNT" 0
spot "$BX" "$BY" "$BZ"
expect "the bloom's spot before the bloom" "$SPOT" 0

echo "== B: an aura bloom stores exactly 150,000 aura, once =="
ctl command "execute in minecraft:overworld run setblock $BX $BY $BZ naturesaura:aura_bloom" >/dev/null
ctl wait 20 >/dev/null
[ "$(block_name "$BX" "$BY" "$BZ")" = "naturesaura:aura_bloom" ] || fail "the bloom did not place"
spot "$BX" "$BY" "$BZ"
expect "the bloom's spot" "$SPOT" "$BLOOM_AURA"
aura "$AX" "$AY" "$AZ" 20
expect "the area's aura with a bloom" "$AREA" "$TOTAL_AURA"
expect "the area's spot sum with a bloom" "$SPOTS" "$BLOOM_AURA"
expect "the area's spot count with a bloom" "$COUNT" 1
ctl wait 60 >/dev/null
spot "$BX" "$BY" "$BZ"
expect "the bloom's spot after 60 more ticks (it generates once)" "$SPOT" "$BLOOM_AURA"

echo "== C: an altar outside a multiblock does nothing =="
ctl command "execute in minecraft:overworld run setblock $CX $CY $CZ naturesaura:nature_altar" >/dev/null
ctl wait 6 >/dev/null
[ "$(block_name "$CX" "$CY" "$CZ")" = "naturesaura:nature_altar" ] || fail "the lone altar did not place"
hold minecraft:iron_ingot 2
click "$CX" "$CY" "$CZ"
altar "$CX" "$CY" "$CZ"
expect "the lone altar's completeness" "$COMPLETE" 0
expect "the lone altar's slot" "$SLOT" "minecraft:iron_ingot x1"
ctl wait 200 >/dev/null
altar "$CX" "$CY" "$CZ"
expect "the lone altar's completeness after 200 ticks" "$COMPLETE" 0
expect "the lone altar's container after 200 ticks" "$ALTAR_AURA" 0
expect "the lone altar's slot after 200 ticks" "$SLOT" "minecraft:iron_ingot x1"
aura "$AX" "$AY" "$AZ" 20
expect "the area's aura with an incomplete altar" "$AREA" "$TOTAL_AURA"
echo "  the altar outside its multiblock took no aura and transmuted nothing"

echo "== D: the complete altar turns an iron ingot into infused iron =="
say "naturesaura altar-build $AX $AY $AZ"
ctl wait 12 >/dev/null
BUILT=$(ctl messages 12 | jq -r '.chat[]' | grep -m1 "frame around" || true)
[ -n "$BUILT" ] || fail "no /naturesaura altar-build reply"
echo "  $BUILT"
case "$BUILT" in
    *"placed 48 blocks"*) ;;
    *) fail "the frame was not built completely: $BUILT" ;;
esac
# Four positions of the jar's pattern, named absolutely: the top gold corners
# (offset ±4, +2, 0), the stone bricks under one (0, +1, -4) and the bottom ring
# (offset -1, -1, -3 planks; -1, -1, -1 fancy bricks).
[ "$(block_name $((AX + 4)) $((AY + 2)) "$AZ")" = "naturesaura:gold_brick" ] || fail "no gold brick at the top corner"
[ "$(block_name $((AX - 4)) $((AY + 2)) "$AZ")" = "naturesaura:gold_brick" ] || fail "no gold brick at the other top corner"
[ "$(block_name "$AX" $((AY + 1)) $((AZ - 4)) )" = "minecraft:stone_bricks" ] || fail "no stone bricks above the bottom layer"
[ "$(block_name $((AX - 1)) $((AY - 1)) $((AZ - 3)) )" = "minecraft:oak_planks" ] || fail "no planks in the bottom ring"
[ "$(block_name $((AX - 1)) $((AY - 1)) $((AZ - 1)) )" = "minecraft:red_nether_bricks" ] || fail "no fancy bricks in the bottom ring"
ctl command "execute in minecraft:overworld run setblock $AX $AY $AZ naturesaura:nature_altar" >/dev/null
ctl wait 20 >/dev/null
altar "$AX" "$AY" "$AZ"
expect "the altar's completeness" "$COMPLETE" 1
# A complete altar pulls from its first tick, so its container is already
# filling; what must be empty is the slot.
expect "the altar's slot before the ritual" "$SLOT" empty
hold minecraft:iron_ingot 2
click "$AX" "$AY" "$AZ"
altar "$AX" "$AY" "$AZ"
expect "the altar's slot" "$SLOT" "minecraft:iron_ingot x1"
echo "  the altar holds $SLOT with $ALTAR_AURA aura, area $AREA"
ctl wait 260 >/dev/null
altar "$AX" "$AY" "$AZ"
expect "the altar's slot after the ritual" "$SLOT" "naturesaura:infused_iron x1"
expect "the altar's recipe after the ritual" "$RECIPE" none
expect "the altar's timer after the ritual" "$TIMER" 0
# Conservation over the whole window: the area lost exactly the recipe's aura,
# and everything else it lost is in the altar's own container.
if [ "$SELFTEST" = 1 ]; then
    echo "(selftest: asserting the area and container add up to $((AREA_AFTER + 1)))"
    expect "area + container (selftest, must fail)" $((AREA + ALTAR_AURA)) $((AREA_AFTER + 1))
else
    expect "area + container" $((AREA + ALTAR_AURA)) "$AREA_AFTER"
fi
echo "  area $AREA + container $ALTAR_AURA = $((AREA + ALTAR_AURA)); the recipe spent $RECIPE_SPEND of $TOTAL_AURA"

echo "== E: an empty hand takes the infused iron off the altar =="
# Slot 2 is empty, so the main hand is empty after selecting it.
ctl select 2 >/dev/null
ctl wait 4 >/dev/null
click "$AX" "$AY" "$AZ"
altar "$AX" "$AY" "$AZ"
expect "the altar's slot after taking" "$SLOT" empty
ctl inventory | jq -e '[.hotbar[], .main[] | select(.item == "naturesaura:infused_iron")] | length >= 1' >/dev/null \
    || fail "the infused iron did not reach the player's inventory"
echo "  the infused iron is in the player's inventory"

echo "== F: an altar in an area with no aura transmutes nothing =="
say "naturesaura drain $AX $AY $AZ 1200000"
ctl wait 10 >/dev/null
aura "$AX" "$AY" "$AZ" 20
[ "$AREA" -lt 0 ] || fail "the area still holds $AREA aura, expected a deficit"
say "naturesaura altar-aura $AX $AY $AZ 0"
ctl wait 10 >/dev/null
hold minecraft:iron_ingot 2
click "$AX" "$AY" "$AZ"
altar "$AX" "$AY" "$AZ"
expect "the altar's slot in an empty area" "$SLOT" "minecraft:iron_ingot x1"
container_before=$ALTAR_AURA
ctl wait 200 >/dev/null
altar "$AX" "$AY" "$AZ"
expect "the altar's container in an area with no aura" "$ALTAR_AURA" "$container_before"
expect "the altar's slot in an area with no aura" "$SLOT" "minecraft:iron_ingot x1"
echo "  with area $AREA the altar took no aura and transmuted nothing"

echo "PASS: a bloom stores exactly 150,000 aura once, an altar outside its multiblock and an altar in an area with no aura transmute nothing, and the complete altar turns an iron ingot into infused iron while the area and its container account for exactly the recipe's 15,040 aura"
