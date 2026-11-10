#!/usr/bin/env python3
"""Assemble slice 1's `content/` folder from the pack's Nature's Aura jar.

The jar is the ATM10 file (`ports/_shared/licence-cache/NaturesAura-41.10.jar`,
SHA-1 `58e4f9a0…b8db`), kept as the licence and provenance evidence. Nothing is
invented: the blockstates, models, textures, lang file, loot tables, crafting
recipes and `altar_*` block tags come out of the jar, re-spelled only where 26.3
differs (the carry pipeline's upgrades: `{"tag": T}` -> `"#T"`, a loot
`conditions` list -> one `condition`). The block and item declarations and the
vanilla tags are written here, because the carry guesses a template and writes
no convention tags without an oracle.

Rerun it to rebuild the carried half of `content/` (it deletes only what it
writes):

    python3 tools/assemble_content.py
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import shutil
import zipfile

ROOT = Path(__file__).resolve().parents[1]
JAR = ROOT.parent / "_shared/licence-cache/NaturesAura-41.10.jar"
CONTENT = ROOT / "content"
MOD = "naturesaura"
# The pack's Modrinth SHA-1 for NaturesAura-41.10.jar (version kJ1hHmK0).
JAR_SHA1 = "58e4f9a07e5320432d11aaa3d876337edeeeb8db"

BLOCKS = ["aura_bloom", "nature_altar", "gold_brick", "infused_stone"]
ITEMS = ["infused_iron", "tainted_gold", "gold_fiber"]

# Carried byte-for-byte from the jar: blockstates, block and item models, the
# four textures families, the lang file, the `altar_*` block tags.
CARRIED = [
    *(f"assets/{MOD}/blockstates/{name}.json" for name in BLOCKS),
    *(f"assets/{MOD}/models/block/{name}.json" for name in BLOCKS),
    *(f"assets/{MOD}/models/item/{name}.json" for name in BLOCKS + ITEMS),
    *(f"assets/{MOD}/textures/block/{name}.png" for name in BLOCKS),
    f"assets/{MOD}/textures/block/nature_altar_top.png",
    *(f"assets/{MOD}/textures/item/{name}.png" for name in ITEMS),
    f"assets/{MOD}/lang/en_us.json",
    *(f"data/{MOD}/tags/block/altar_{kind}.json" for kind in ("wood", "stone", "gold_brick", "fancy_brick")),
]

# Loot tables and crafting recipes are carried through the upgrade functions.
LOOT = [f"data/{MOD}/loot_table/blocks/{name}.json" for name in BLOCKS]
RECIPES = [f"data/{MOD}/recipe/{name}.json" for name in ("gold_fiber", "gold_brick")]

# 26.3's item model definition: the jar's own `models/item` file, named by it.
def item_definition(name: str) -> dict:
    return {"model": {"type": "minecraft:model", "model": f"{MOD}:item/{name}"}}


# The blocks, from `reg/ModRegistry` and their classes:
#  - `aura_bloom` is `BlockAuraBloom("aura_bloom", GRASS_BLOCK, DIRT, PODZOL,
#    COARSE_DIRT, FARMLAND)`: `Properties.of().noCollission().strength(0)
#    .sound(SoundType.GRASS)`, a bush with a `SHAPE` of 5..11 x 0..10 x 5..11.
#    `minecraft:poppy` is the pin's bush template; its ground set is vanilla's
#    dirt-like list (see notes.md for the deviation).
#  - `nature_altar` is `BlockNatureAltar`: `Properties.of().strength(4F)`, a
#    12/16-high `SHAPE`, waterloggable (not carried: the templates here have no
#    waterlogged state).
#  - `gold_brick` is `BlockImpl("gold_brick", ofFullCopy(STONE_BRICKS))`.
#  - `infused_stone` is `BlockImpl("infused_stone", sound(STONE).strength(1.75F))`.
BLOCK_DECLARATIONS = {
    "aura_bloom": {
        "template": "minecraft:poppy",
        "properties": {
            "hardness": 0.0,
            "explosion_resistance": 0.0,
            "sounds": "minecraft:grass_block",
            "requires_tool": False,
        },
        "shapes": {"default": [[5 / 16, 0.0, 5 / 16, 11 / 16, 10 / 16, 11 / 16]]},
        "collision_shapes": {"default": []},
    },
    "nature_altar": {
        "template": "minecraft:stone",
        "properties": {
            "hardness": 4.0,
            "explosion_resistance": 4.0,
            "sounds": "minecraft:stone",
            "occludes": False,
            "requires_tool": True,
        },
        "shapes": {"default": [[0.0, 0.0, 0.0, 1.0, 12 / 16, 1.0]]},
        "collision_shapes": {"default": [[0.0, 0.0, 0.0, 1.0, 12 / 16, 1.0]]},
    },
    "gold_brick": {
        "template": "minecraft:stone_bricks",
        "properties": {
            "hardness": 1.5,
            "explosion_resistance": 6.0,
            "sounds": "minecraft:stone",
            "requires_tool": True,
        },
    },
    "infused_stone": {
        "template": "minecraft:stone",
        "properties": {
            "hardness": 1.75,
            "explosion_resistance": 1.75,
            "sounds": "minecraft:stone",
            "requires_tool": True,
        },
    },
}

# The three plain materials (`ModItems`: `new ItemBase("infused_iron")` …).
ITEM_DECLARATIONS = {name: {"template": "minecraft:iron_ingot"} for name in ITEMS}

# Upstream's own item group (`itemGroup.naturesaura`), listing the slice's
# items only.
TAB = {
    "title": "item_group.naturesaura.tab",
    "icon": f"{MOD}:nature_altar",
    "items": [f"{MOD}:{name}" for name in BLOCKS + ITEMS],
}

# The jar's own tag files, filtered to the slice's blocks: pickaxe mineable
# (the altar, gold brick and infused stone) and small flowers (the bloom).
MINEABLE_PICKAXE = {"values": [f"{MOD}:nature_altar", f"{MOD}:gold_brick", f"{MOD}:infused_stone"]}
SMALL_FLOWERS = {"values": [f"{MOD}:aura_bloom"]}

# The `gold_fiber` recipe names `c:nuggets/gold`, which the prepared 26.3 root
# does not ship; an unknown tag is an empty ingredient, so it is written.
CONVENTION_TAGS = {"data/c/tags/item/nuggets/gold.json": {"values": ["minecraft:gold_nugget"]}}

MANIFEST = f"""# Port-owned content mod for the Nature's Aura 41.10 slice: the aura bloom, the
# nature's altar, its gold bricks and the three materials the no-catalyst altar
# recipes make. The models, textures, blockstates, lang file, loot tables,
# recipes and altar_* tags are upstream's own MIT assets, read from the exact
# ATM10 jar (see ../FINDINGS.md §1 and ../../_shared/carry-work/{MOD}/SOURCE.md).
id = "{MOD}"
name = "Nature's Aura"
version = "0.1.0"
api = "0.2"
side = "both"
capabilities = ["registries"]
"""


def upgrade_recipe(recipe: dict) -> dict:
    """The carry pipeline's 1.21.1 -> 26.3 ingredient upgrade."""
    for key, value in list(recipe.get("key", {}).items()):
        if isinstance(value, dict) and "tag" in value:
            recipe["key"][key] = f"#{value['tag']}"
        elif isinstance(value, dict) and "item" in value:
            recipe["key"][key] = value["item"]
    ingredients = recipe.get("ingredients")
    if isinstance(ingredients, list):
        recipe["ingredients"] = [
            f"#{value['tag']}" if isinstance(value, dict) and "tag" in value else value
            for value in ingredients
        ]
    return recipe


def upgrade_loot(loot: dict) -> dict:
    """The carry pipeline's loot upgrade: one `condition` object per pool."""
    for pool in loot.get("pools", []):
        conditions = pool.pop("conditions", None)
        if conditions:
            condition = dict(conditions[0])
            condition["type"] = f"minecraft:{condition.pop('condition').split(':')[-1]}"
            pool["condition"] = condition
    return loot


def write(relative: str, data) -> None:
    path = CONTENT / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(data, bytes):
        path.write_bytes(data)
    else:
        path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")


def main() -> None:
    raw = JAR.read_bytes()
    digest = hashlib.sha1(raw).hexdigest()
    if digest != JAR_SHA1:
        raise SystemExit(f"jar SHA-1 {digest} is not the pack record {JAR_SHA1}")
    archive = zipfile.ZipFile(JAR)

    # Remove only what this script owns, so a hand-added patches/ file survives.
    for folder in ("assets", "data"):
        shutil.rmtree(CONTENT / folder, ignore_errors=True)

    for name in CARRIED:
        write(name, archive.read(name))
    for name in LOOT:
        write(name, upgrade_loot(json.loads(archive.read(name))))
    for name in RECIPES:
        write(name, upgrade_recipe(json.loads(archive.read(name))))

    for name in BLOCKS + ITEMS:
        write(f"assets/{MOD}/items/{name}.json", item_definition(name))
    for name, declaration in BLOCK_DECLARATIONS.items():
        write(f"data/{MOD}/horizon/block/{name}.json", declaration)
    for name, declaration in ITEM_DECLARATIONS.items():
        write(f"data/{MOD}/horizon/item/{name}.json", declaration)
    write(f"data/{MOD}/horizon/creative_tab/natures_aura.json", TAB)
    write("data/minecraft/tags/block/mineable/pickaxe.json", MINEABLE_PICKAXE)
    write("data/minecraft/tags/block/small_flowers.json", SMALL_FLOWERS)
    for relative, data in CONVENTION_TAGS.items():
        write(relative, data)
    (CONTENT / "horizon-mod.toml").write_text(MANIFEST)

    files = sorted(p for p in CONTENT.rglob("*") if p.is_file())
    print(f"{len(files)} files under {CONTENT}")
    for path in files:
        print(" ", path.relative_to(CONTENT))


if __name__ == "__main__":
    main()
