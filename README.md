# naturesaura (Horizon port)

A Horizon port of [naturesaura](Ellpeck/NaturesAura) — MIT.

**Status: first slices.** What is below is what the port actually does today.

## Implemented so far

- Implemented: the chunk aura drain spots (base 1,000,000 + spots), the aura bloom
- (150,000 aura, once, when it appears), the nature's altar's 9x4x9 multiblock, its
- ,000 container (300 aura a tick from the area, only in the Overworld and the
- Nether), the ritual for the three catalyst-free recipes, the right-click
- insert/take, and the materials that make the multiblock buildable in-game.
- Why this is the smallest honest loop: the altar's only inputs are aura and one
- item, its only output one item, so the loop closes with no GUI, no energy, no
- fluid and no multiblock-machine API; yet every piece of it is the mod's own core

## Not yet ported

- | Gap | Shape | What the slice does instead |
- |---|---|---|
- | `IAuraType` per dimension | the pin has no aura-type registry | `game.native_dimension()`: the altar pulls in `minecraft:overworld` and `minecraft:nether`, and not in the End — upstream's rule, no type objects |
- | Aura spot storage | NeoForge attaches to `LevelChunk`; the pin has `Holder::Chunk` attachments | a `ChunkAura` codec compound `{spots: [{x,y,z,amount}]}`; same arithmetic, same NBT spirit |
- | `PacketAuraChunk` / client aura overlay | no client HUD in the slice | `sync: false`; the readouts are commands. A later slice adds the overlay and then needs `sync: true` |
- | The altar's waterlogging | the slice's block templates have no waterlogged state | not carried (`BlockNatureAltar.hasWaterlogging`) |
- | The bloom's ground set | `minecraft:poppy` allows mycelium, moss, mud and rooted dirt too | upstream's five grounds are covered; the four extra ones are a template deviation |
- | Aura-container items | the altar's "fill the held item's aura" branch | not in the slice (no such items carried); the tick goes straight to the recipe |

## Install

Build the bundle with a native-mod capable launcher, or add the file from the
registry once it is listed. Pinned to Horizon `1d63e0a` and horizon-mixins
`5a64af93` for game 26.3.

## Build from source

```sh
cargo test
python3 tools/horizon.py check --horizon <horizon checkout>
python3 tools/horizon.py build --horizon <horizon checkout>
python3 tools/package.py
```

## Credits and licence

Original mod: Ellpeck/NaturesAura. Port: AdLf0208. Licence: MIT — see [LICENSE](LICENSE);
the original notices are preserved.
