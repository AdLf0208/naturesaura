// Game-side glue for Nature's Aura slice 1.
//
// Injected into `crate::game` as `naturesaura_state`; it needs the game, so it
// is not part of the standalone library (only `runtime.rs` is).
//
// What it does: keeps the aura drain spots of a chunk in a chunk attachment
// (`Holder::Chunk`, the shape `AuraChunk` saves), runs the aura bloom's one-time
// generation and the nature's altar's tick (multiblock check, 300 aura a tick
// out of the area, then the recipe), and answers the click, the harness
// commands and the embedded content mod.
//
// Read from Nature's Aura 41.10: `chunk/AuraChunk`, `InternalHooks`
// (`getAuraInArea`/`getLowestAuraDrainSpot`/`getHighestAuraDrainSpot`),
// `BlockEntityImpl.generateAura`, `BlockEntityAuraBloom`,
// `BlockEntityNatureAltar`, `blocks/multi/Multiblock`, `Multiblocks.ALTAR`,
// `Helper.putStackOnTile`. See `../../FINDINGS.md` and `../../notes.md`.

use std::array;

use horizon_client::containers::{BlockEntity, ModBlockEntity, ModContainer};
use horizon_client::items::ItemStack;
use horizon_client::mod_api::{AttachmentCodec, AttachmentError, AttachmentType, Holder};
use horizon_client::text::Text;
use horizon_world::level::flags;
use horizon_world::nbt::{Compound, JavaString, Tag};

use crate::game::Game;
use crate::game::naturesaura::{
    ALTAR_CHECK_TICKS, ALTAR_MAX_AURA, ALTAR_RADIUS, Altar, AltarPart, BLOOM_GENERATED_AURA,
    ChunkAura, DEFAULT_AURA, GENERATE_RADIUS, altar_complete, altar_parts, altar_recipe,
};
use crate::game::native_server::{NativeCommand, NativeServerPlayer};

/// The aura bloom (`ModRegistry` `BlockAuraBloom("aura_bloom", …)`).
pub const BLOOM: &str = "naturesaura:aura_bloom";
/// The nature's altar (`BlockNatureAltar`).
pub const ALTAR: &str = "naturesaura:nature_altar";
/// The altar's one item slot (`ItemStackHandlerNA(1, this, true)`).
const ALTAR_SLOT: usize = 0;
const SLOT_COUNT: usize = 1;
/// The altar's saved keys (`BlockEntityNatureAltar.writeNBT`, plus the cached
/// multiblock answer, which upstream keeps as `complete`).
const KEY_AURA: &str = "aura";
const KEY_TIMER: &str = "timer";
const KEY_RECIPE: &str = "recipe";
const KEY_COMPLETE: &str = "complete";
/// The bloom's saved flag (`BlockEntityAuraBloom.just_generated`).
const KEY_JUST_GENERATED: &str = "just_generated";
/// `recipe = -1` and `complete = -1` in a fresh block entity: not decided yet.
const UNSET: i32 = -1;

// ---------------------------------------------------------------------------
// The chunk attachment
// ---------------------------------------------------------------------------

/// The drain spots of a chunk, saved as `AuraChunk.serializeNBT` saves them
/// (`{drain_spots: [{pos, amount}]}`), here as `{spots: [{x, y, z, amount}]}`
/// because the pin writes NBT compounds, not packed longs.
///
/// `sync: false`: the aura is a server-side fact; the slice has no client HUD
/// that shows it (upstream sends `PacketAuraChunk`; that is a later slice).
static AURA: AttachmentType<ChunkAura> = AttachmentType {
    namespace: "naturesaura",
    name: "aura",
    default: ChunkAura::new,
    codec: Some(AttachmentCodec {
        encode: encode_aura,
        decode: decode_aura,
    }),
    copy_on_death: false,
    sync: false,
};

fn encode_aura(chunk: &ChunkAura) -> Result<Compound, AttachmentError> {
    let spots = chunk
        .spots()
        .map(|(pos, amount)| {
            let mut spot = Compound::new();
            spot.insert("x".into(), Tag::Int(pos[0]));
            spot.insert("y".into(), Tag::Int(pos[1]));
            spot.insert("z".into(), Tag::Int(pos[2]));
            spot.insert("amount".into(), Tag::Int(amount));
            Tag::Compound(spot)
        })
        .collect();
    let mut out = Compound::new();
    out.insert("spots".into(), Tag::List(spots));
    Ok(out)
}

fn decode_aura(data: &Compound) -> Result<ChunkAura, AttachmentError> {
    let Some(Tag::List(list)) = data.get(&JavaString::from("spots")) else {
        return Ok(ChunkAura::new());
    };
    let mut spots = Vec::with_capacity(list.len());
    for entry in list {
        let Tag::Compound(spot) = entry else {
            return Err(AttachmentError::Decode);
        };
        let (Some(x), Some(y), Some(z), Some(amount)) = (
            int_of(spot, "x"),
            int_of(spot, "y"),
            int_of(spot, "z"),
            int_of(spot, "amount"),
        ) else {
            return Err(AttachmentError::Decode);
        };
        spots.push(([x, y, z], amount));
    }
    Ok(ChunkAura::from_spots(spots))
}

fn int_of(compound: &Compound, key: &str) -> Option<i32> {
    match compound.get(&JavaString::from(key)) {
        Some(Tag::Int(value)) => Some(*value),
        _ => None,
    }
}

/// The chunks a block position and radius touch, in a stable order.
fn chunks_in_radius(center: [i32; 3], radius: i32) -> Vec<[i32; 2]> {
    let (x0, x1) = (center[0] - radius, center[0] + radius);
    let (z0, z1) = (center[2] - radius, center[2] + radius);
    let mut chunks = Vec::new();
    for cx in (x0 >> 4)..=(x1 >> 4) {
        for cz in (z0 >> 4)..=(z1 >> 4) {
            chunks.push([cx, cz]);
        }
    }
    chunks
}

/// The aura of the area: the base [`DEFAULT_AURA`] plus every drain spot within
/// `radius` (`InternalHooks.getAuraInArea`). Chunks that are not loaded hold no
/// spots, so they add nothing.
fn area_aura(game: &mut Game<'_>, center: [i32; 3], radius: i32) -> i32 {
    let mut spots = 0i32;
    for chunk in chunks_in_radius(center, radius) {
        let Ok(store) = game
            .native_attachments(Holder::Chunk(chunk))
            .and_then(|attachments| attachments.get(&AURA))
        else {
            continue;
        };
        spots = spots.saturating_add(store.sum_in(center, radius));
    }
    DEFAULT_AURA.saturating_add(spots)
}

/// The area's spot sum and count in one walk, for the readout.
fn area_spots(game: &mut Game<'_>, center: [i32; 3], radius: i32) -> (i32, usize) {
    let (mut sum, mut count) = (0i32, 0usize);
    for chunk in chunks_in_radius(center, radius) {
        let Ok(store) = game
            .native_attachments(Holder::Chunk(chunk))
            .and_then(|attachments| attachments.get(&AURA))
        else {
            continue;
        };
        sum = sum.saturating_add(store.sum_in(center, radius));
        count += store.count_in(center, radius);
    }
    (sum, count)
}

/// The fullest spot in radius (`getHighestAuraDrainSpot`), if any holds aura.
fn highest_spot(game: &mut Game<'_>, center: [i32; 3], radius: i32) -> Option<[i32; 3]> {
    let mut best: Option<([i32; 3], i32)> = None;
    for chunk in chunks_in_radius(center, radius) {
        let Ok(store) = game
            .native_attachments(Holder::Chunk(chunk))
            .and_then(|attachments| attachments.get(&AURA))
        else {
            continue;
        };
        if let Some(candidate) = store.highest_in(center, radius)
            && best.is_none_or(|(_, amount)| candidate.1 > amount)
        {
            best = Some(candidate);
        }
    }
    best.map(|(pos, _)| pos)
}

/// The most indebted spot in radius (`getLowestAuraDrainSpot`), if any is in
/// debt.
fn lowest_spot(game: &mut Game<'_>, center: [i32; 3], radius: i32) -> Option<[i32; 3]> {
    let mut best: Option<([i32; 3], i32)> = None;
    for chunk in chunks_in_radius(center, radius) {
        let Ok(store) = game
            .native_attachments(Holder::Chunk(chunk))
            .and_then(|attachments| attachments.get(&AURA))
        else {
            continue;
        };
        if let Some(candidate) = store.lowest_in(center, radius)
            && best.is_none_or(|(_, amount)| candidate.1 < amount)
        {
            best = Some(candidate);
        }
    }
    best.map(|(pos, _)| pos)
}

/// Drains one spot (`IAuraChunk.drainAura(pos, amount)`), answering what moved.
fn drain_at(game: &mut Game<'_>, pos: [i32; 3], amount: i32) -> i32 {
    let chunk = [pos[0] >> 4, pos[2] >> 4];
    let Ok(mut store) = game
        .native_attachments(Holder::Chunk(chunk))
        .and_then(|attachments| attachments.get(&AURA))
    else {
        return 0;
    };
    let drained = store.drain(pos, amount, false);
    if drained > 0 {
        let _ = game
            .native_attachments(Holder::Chunk(chunk))
            .and_then(|attachments| attachments.set(&AURA, store));
    }
    drained
}

/// Stores into one spot (`IAuraChunk.storeAura(pos, amount)`), answering what
/// went in.
fn store_at(game: &mut Game<'_>, pos: [i32; 3], amount: i32) -> i32 {
    let chunk = [pos[0] >> 4, pos[2] >> 4];
    let Ok(mut store) = game
        .native_attachments(Holder::Chunk(chunk))
        .and_then(|attachments| attachments.get(&AURA))
    else {
        return 0;
    };
    let stored = store.store(pos, amount, true);
    if stored > 0 {
        let _ = game
            .native_attachments(Holder::Chunk(chunk))
            .and_then(|attachments| attachments.set(&AURA, store));
    }
    stored
}

/// What one spot holds, for the readout and the harness.
fn amount_at(game: &mut Game<'_>, pos: [i32; 3]) -> i32 {
    game.native_attachments(Holder::Chunk([pos[0] >> 4, pos[2] >> 4]))
        .and_then(|attachments| attachments.get(&AURA))
        .map_or(0, |store| store.amount_at(pos))
}

/// `BlockEntityImpl.generateAura`: fill the emptiest spot in radius 35, then the
/// next one, until the aura is placed (a bounded number of spots, as Java's
/// `while` also ends because every store takes what it is given).
fn generate_aura(game: &mut Game<'_>, pos: [i32; 3], amount: i32) {
    let mut left = amount;
    for _ in 0..8 {
        if left <= 0 {
            break;
        }
        let spot = lowest_spot(game, pos, GENERATE_RADIUS).unwrap_or(pos);
        let stored = store_at(game, spot, left);
        if stored <= 0 {
            break;
        }
        left -= stored;
    }
}

/// Whether the altar may pull aura here: upstream pulls only when the level's
/// aura type is the overworld's or the nether's (`IAuraType.forLevel`).
fn altar_pulls_here(game: &Game<'_>) -> bool {
    matches!(game.native_dimension(), "minecraft:overworld" | "minecraft:nether")
}

// ---------------------------------------------------------------------------
// Declaration hooks
// ---------------------------------------------------------------------------

/// A block entity for the bloom and the altar (`native_block_entity`, API 15 /
/// 129: "whatever its template").
#[allow(
    private_interfaces,
    reason = "the injected bundle is this crate's own module"
)]
pub fn on_block_entity(block: &str, entity: &mut Option<(String, Compound)>) {
    match block {
        BLOOM => {
            let mut nbt = Compound::new();
            nbt.insert(KEY_JUST_GENERATED.into(), Tag::Int(1));
            *entity = Some((block.to_owned(), nbt));
        }
        ALTAR => {
            let mut nbt = Compound::new();
            nbt.insert(KEY_AURA.into(), Tag::Int(0));
            nbt.insert(KEY_TIMER.into(), Tag::Int(0));
            nbt.insert(KEY_RECIPE.into(), Tag::Int(UNSET));
            nbt.insert(KEY_COMPLETE.into(), Tag::Int(UNSET));
            *entity = Some((block.to_owned(), nbt));
        }
        _ => {}
    }
}

/// Both blocks tick on the server (`native_block_entity_ticks`, API 15).
pub fn on_block_entity_ticks(block: &str, ticks: &mut bool) {
    if block == BLOOM || block == ALTAR {
        *ticks = true;
    }
}

/// The altar's one slot (`native_block_entity_container`, API 16/17).
///
/// Upstream's handler limits the slot to one item and refuses an insert that is
/// not a recipe input; the pin cannot express a per-item rule, so every side
/// reaches the slot and the click and the tick enforce the item rules (a gap
/// recorded in `notes.md`).
#[allow(
    private_interfaces,
    reason = "the injected bundle is this crate's own module"
)]
pub fn on_container(block: &str, container: &mut Option<ModContainer>) {
    if block != ALTAR {
        return;
    }
    *container = Some(ModContainer {
        slots: SLOT_COUNT,
        faces: array::from_fn(|_| vec![ALTAR_SLOT]),
        inputs: vec![ALTAR_SLOT],
        outputs: vec![ALTAR_SLOT],
        menu: None,
    });
}

// ---------------------------------------------------------------------------
// The tick
// ---------------------------------------------------------------------------

/// One server tick of the two blocks (`native_block_entity_tick`, API 17).
#[allow(
    private_interfaces,
    reason = "the injected bundle is this crate's own module"
)]
pub fn on_tick(game: &mut Game<'_>, pos: [i32; 3], entity: &mut ModBlockEntity) {
    match entity.id.as_str() {
        BLOOM => bloom_tick(game, pos, entity),
        ALTAR => altar_tick(game, pos, entity),
        _ => {}
    }
}

/// The bloom's one-time generation (`BlockEntityAuraBloom.tick`): the first tick
/// after it appears stores 150,000 aura at the emptiest spot in radius 35 and
/// clears the flag for good.
fn bloom_tick(game: &mut Game<'_>, pos: [i32; 3], entity: &mut ModBlockEntity) {
    if read_int(&entity.nbt, KEY_JUST_GENERATED) == 0 {
        return;
    }
    write_int(&mut entity.nbt, KEY_JUST_GENERATED, 0);
    generate_aura(game, pos, BLOOM_GENERATED_AURA);
}

/// The altar's tick (`BlockEntityNatureAltar.tick`, minus the aura-container
/// item branch, which needs items this slice does not carry).
fn altar_tick(game: &mut Game<'_>, pos: [i32; 3], entity: &mut ModBlockEntity) {
    if entity.items.len() < SLOT_COUNT {
        entity.items.resize(SLOT_COUNT, None);
    }
    let mut altar = Altar::new();
    altar.aura = read_int(&entity.nbt, KEY_AURA).clamp(0, ALTAR_MAX_AURA);
    altar.timer = read_int(&entity.nbt, KEY_TIMER).max(0);
    altar.recipe = match read_int(&entity.nbt, KEY_RECIPE) {
        UNSET => None,
        index if index >= 0 => Some(index as usize),
        _ => None,
    };
    let cached = read_int(&entity.nbt, KEY_COMPLETE);
    let complete = if cached == UNSET || game.world.game_time % ALTAR_CHECK_TICKS == 0 {
        let answer = altar_complete(pos, |at| game.native_block(at));
        write_int(&mut entity.nbt, KEY_COMPLETE, i32::from(answer));
        answer
    } else {
        cached == 1
    };

    let mut crafted = None;
    if complete {
        let room = altar.pull_room();
        if room > 0 && altar_pulls_here(game) {
            // `toStore = min(area aura, room)`: a drained area can answer a
            // negative amount, and then nothing is pulled.
            let area = area_aura(game, pos, ALTAR_RADIUS);
            let to_store = area.min(room);
            if to_store > 0 {
                let spot = highest_spot(game, pos, ALTAR_RADIUS).unwrap_or(pos);
                altar.take_pulled(drain_at(game, spot, to_store));
            }
        }
        let slot = entity.items[ALTAR_SLOT]
            .as_ref()
            .map(|stack| stack.item.to_string());
        if let Some(recipe) = altar.step(slot.as_deref()) {
            entity.items[ALTAR_SLOT] = Some(ItemStack::new(recipe.output, recipe.count));
            crafted = Some(recipe.output);
        }
    }

    write_int(&mut entity.nbt, KEY_AURA, altar.aura);
    write_int(&mut entity.nbt, KEY_TIMER, altar.timer);
    write_int(
        &mut entity.nbt,
        KEY_RECIPE,
        altar.recipe.map_or(UNSET, |index| index as i32),
    );
    // `setChanged`: the chunk is saved with what this tick did.
    game.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
    if crafted.is_some() {
        game.native_block_entity_sync(pos);
    }
}

// ---------------------------------------------------------------------------
// The altar click
// ---------------------------------------------------------------------------

/// A right click on the altar (`native_right_click_block`, API 21),
/// `Helper.putStackOnTile`: put one held item on the altar, or take what is on
/// it into the player's inventory.
#[allow(
    private_interfaces,
    reason = "the injected bundle is this crate's own module"
)]
pub fn on_right_click(game: &mut Game<'_>, pos: [i32; 3], _face: [i32; 3], canceled: &mut bool) {
    let block = game.native_block(pos);
    if block != ALTAR {
        log::info!("[naturesaura] right click at {pos:?} on {block}, not the altar");
        return;
    }
    *canceled = true;
    let held = game.held().cloned();
    let slot = altar_slot(game, pos);
    log::info!(
        "[naturesaura] altar click at {pos:?}: held={:?} slot={:?}",
        held.as_ref().map(|stack| stack.item.to_string()),
        slot.as_ref().map(|stack| stack.item.to_string())
    );
    let inserted = match (&held, &slot) {
        (Some(stack), _) if stack.count > 0 && altar_recipe(&stack.item).is_some() && slot.is_none() => {
            let one = ItemStack::new(&stack.item, 1);
            let placed = game
                .native_with_mod_block_entity(pos, |entity| {
                    if entity.items.len() < SLOT_COUNT {
                        entity.items.resize(SLOT_COUNT, None);
                    }
                    entity.items[ALTAR_SLOT] = Some(one);
                })
                .is_some();
            if placed && !game.creative {
                let _ = game.native_take_items(&stack.item, 1);
            }
            placed
        }
        _ => false,
    };
    if inserted {
        return;
    }
    // Upstream's handler refuses an input through `canExtract`, and the game
    // asks this hook for the second hand of the same click: taking only a
    // finished product keeps that second ask from undoing the insert above.
    let Some(stack) = altar_slot(game, pos) else {
        return;
    };
    if altar_recipe(&stack.item).is_some() {
        return;
    }
    let Some(player) = game.native_acting_player() else {
        return;
    };
    let left = game
        .native_give_stack(player.uuid, stack.clone())
        .filter(|left| left.count > 0);
    let _ = game.native_with_mod_block_entity(pos, |entity| {
        entity.items[ALTAR_SLOT] = None;
    });
    if let Some(left) = left {
        game.pop_resource(pos, left);
    }
}

/// The altar's slot at `pos`, if that block entity is one.
fn altar_slot(game: &Game<'_>, pos: [i32; 3]) -> Option<ItemStack> {
    match game.world.block_entities.get(&(pos[0], pos[1], pos[2])) {
        Some(BlockEntity::Mod(entity)) if entity.id == ALTAR => {
            entity.items.get(ALTAR_SLOT).and_then(Option::clone)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The `/naturesaura` readouts and harness controls
// ---------------------------------------------------------------------------

/// The commands (`native_register_commands`, API 8). Upstream's own
/// `/naturesaura` command manages aura (add/drain) — these read the slice's
/// state and give the harness the same reach.
#[allow(
    private_interfaces,
    reason = "the injected bundle is this crate's own module"
)]
pub fn on_register_commands(commands: &mut Vec<NativeCommand>) {
    commands.push(NativeCommand::new("naturesaura aura <pos:block_pos> [radius:integer]"));
    commands.push(NativeCommand::new("naturesaura spot <pos:block_pos>"));
    commands.push(NativeCommand::new("naturesaura altar <pos:block_pos>"));
    commands.push(NativeCommand::op("naturesaura altar-build <pos:block_pos>", 2));
    commands.push(NativeCommand::op(
        "naturesaura altar-aura <pos:block_pos> <amount:integer>",
        2,
    ));
    commands.push(NativeCommand::op(
        "naturesaura drain <pos:block_pos> <amount:integer>",
        2,
    ));
}

/// `/naturesaura aura|spot|altar|altar-build|drain`
/// (`native_perform_command`, API 8).
#[allow(
    private_interfaces,
    reason = "the injected bundle is this crate's own module"
)]
pub fn on_perform_command(
    game: &mut Game<'_>,
    _player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    let words: Vec<&str> = command.split_whitespace().collect();
    if words.first() != Some(&"naturesaura") {
        return;
    }
    *ran = true;
    match words.get(1).copied() {
        Some("aura") => {
            let Some(pos) = parse_pos(&words[2..]) else {
                game.native_feedback(Text::literal(
                    "usage: /naturesaura aura <x> <y> <z> [radius]",
                ));
                return;
            };
            let radius = words
                .get(5)
                .and_then(|word| word.parse::<i32>().ok())
                .unwrap_or(ALTAR_RADIUS)
                .clamp(0, 128);
            let (spots, count) = area_spots(game, pos, radius);
            let area = DEFAULT_AURA.saturating_add(spots);
            game.native_feedback(Text::literal(format!(
                "Nature's Aura at {}, {}, {} (radius {radius}): area={area} base={DEFAULT_AURA} spots={spots} count={count}",
                pos[0], pos[1], pos[2]
            )));
        }
        Some("spot") => {
            let Some(pos) = parse_pos(&words[2..]) else {
                game.native_feedback(Text::literal("usage: /naturesaura spot <x> <y> <z>"));
                return;
            };
            let amount = amount_at(game, pos);
            game.native_feedback(Text::literal(format!(
                "Aura spot at {}, {}, {}: {amount}",
                pos[0], pos[1], pos[2]
            )));
        }
        Some("altar") => {
            let Some(pos) = parse_pos(&words[2..]) else {
                game.native_feedback(Text::literal("usage: /naturesaura altar <x> <y> <z>"));
                return;
            };
            let line = altar_readout(game, pos);
            game.native_feedback(Text::literal(line));
        }
        Some("altar-build") => {
            let Some(pos) = parse_pos(&words[2..]) else {
                game.native_feedback(Text::literal(
                    "usage: /naturesaura altar-build <x> <y> <z>",
                ));
                return;
            };
            let placed = build_altar(game, pos);
            let (spots, _) = area_spots(game, pos, ALTAR_RADIUS);
            game.native_feedback(Text::literal(format!(
                "Nature's Altar frame around {}, {}, {}: placed {placed} blocks; area={}",
                pos[0],
                pos[1],
                pos[2],
                DEFAULT_AURA.saturating_add(spots)
            )));
        }
        Some("altar-aura") => {
            let Some((pos, amount)) = parse_pos_amount(&words[2..]) else {
                game.native_feedback(Text::literal(
                    "usage: /naturesaura altar-aura <x> <y> <z> <amount>",
                ));
                return;
            };
            let amount = amount.clamp(0, ALTAR_MAX_AURA);
            let changed = game
                .native_with_mod_block_entity(pos, |entity| {
                    if entity.id != ALTAR {
                        return false;
                    }
                    write_int(&mut entity.nbt, KEY_AURA, amount);
                    true
                })
                .unwrap_or(false);
            let line = if changed {
                format!(
                    "Nature's Altar at {}, {}, {}: the container holds {amount}/{ALTAR_MAX_AURA} aura.",
                    pos[0], pos[1], pos[2]
                )
            } else {
                format!("{pos:?} is not a nature's altar.")
            };
            game.native_feedback(Text::literal(line));
        }
        Some("drain") => {
            let Some((pos, amount)) = parse_pos_amount(&words[2..]) else {
                game.native_feedback(Text::literal(
                    "usage: /naturesaura drain <x> <y> <z> <amount>",
                ));
                return;
            };
            let drained = drain_at(game, pos, amount.max(0));
            let left = amount_at(game, pos);
            let (spots, _) = area_spots(game, pos, ALTAR_RADIUS);
            game.native_feedback(Text::literal(format!(
                "Aura spot at {}, {}, {}: drained {drained}, {left} left; area={}",
                pos[0],
                pos[1],
                pos[2],
                DEFAULT_AURA.saturating_add(spots)
            )));
        }
        _ => game.native_feedback(Text::literal(
            "usage: /naturesaura aura|spot|altar|altar-build|drain ...",
        )),
    }
}

/// The altar's state as `/naturesaura altar` prints it: whether its multiblock
/// is complete, its container, its recipe, its slot and the aura of the area.
fn altar_readout(game: &mut Game<'_>, pos: [i32; 3]) -> String {
    let key = (pos[0], pos[1], pos[2]);
    let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get(&key) else {
        return format!("{pos:?} is not a mod block entity.");
    };
    if entity.id != ALTAR {
        return format!("{pos:?} is a {} not a nature's altar.", entity.id);
    }
    let aura = read_int(&entity.nbt, KEY_AURA).clamp(0, ALTAR_MAX_AURA);
    let timer = read_int(&entity.nbt, KEY_TIMER).max(0);
    let recipe = match read_int(&entity.nbt, KEY_RECIPE) {
        UNSET => "none".to_owned(),
        index if index >= 0 => altar_recipe_name(index as usize),
        _ => "none".to_owned(),
    };
    let slot = match entity.items.get(ALTAR_SLOT).and_then(Option::as_ref) {
        Some(stack) => format!("{} x{}", stack.item, stack.count),
        None => "empty".to_owned(),
    };
    let complete = read_int(&entity.nbt, KEY_COMPLETE) == 1;
    let (spots, _) = area_spots(game, pos, ALTAR_RADIUS);
    format!(
        "Nature's Altar at {}, {}, {}: complete={} aura={aura}/{ALTAR_MAX_AURA} timer={timer} recipe={recipe} slot={slot} area={}",
        pos[0],
        pos[1],
        pos[2],
        i32::from(complete),
        DEFAULT_AURA.saturating_add(spots)
    )
}

fn altar_recipe_name(index: usize) -> String {
    crate::game::naturesaura::ALTAR_RECIPES
        .get(index)
        .map_or_else(|| "none".to_owned(), |recipe| recipe.input.to_owned())
}

/// `/naturesaura altar-build` (harness only): place every matched block of the
/// altar multiblock around `pos`, but not the altar itself, so a scenario can
/// build the structure the way a player would without 48 typed setblocks.
fn build_altar(game: &mut Game<'_>, pos: [i32; 3]) -> usize {
    let dimension = game.native_dimension().to_owned();
    let mut placed = 0;
    for (offset, part) in altar_parts() {
        let Some(id) = multiblock_block(part) else {
            continue;
        };
        let at = [pos[0] + offset[0], pos[1] + offset[1], pos[2] + offset[2]];
        let Some(state) = game.pack.assets.registry.default_state(id) else {
            continue;
        };
        if game.native_set_block(&dimension, at, state, flags::UPDATE_ALL) {
            placed += 1;
        }
    }
    placed
}

/// The block a part of the altar is built from: the first id each jar tag lists.
fn multiblock_block(part: AltarPart) -> Option<&'static str> {
    match part {
        AltarPart::Altar => None,
        AltarPart::Stone => Some("minecraft:stone_bricks"),
        AltarPart::Wood => Some("minecraft:oak_planks"),
        AltarPart::Fancy => Some("minecraft:red_nether_bricks"),
        AltarPart::Gold => Some("naturesaura:gold_brick"),
    }
}

fn parse_pos(words: &[&str]) -> Option<[i32; 3]> {
    Some([
        words.first()?.parse().ok()?,
        words.get(1)?.parse().ok()?,
        words.get(2)?.parse().ok()?,
    ])
}

fn parse_pos_amount(words: &[&str]) -> Option<([i32; 3], i32)> {
    let pos = parse_pos(words)?;
    let amount = words.get(3)?.parse().ok()?;
    Some((pos, amount))
}

// ---------------------------------------------------------------------------
// Saved-compound reads
// ---------------------------------------------------------------------------

fn read_int(nbt: &Compound, key: &str) -> i32 {
    int_of(nbt, key).unwrap_or(0)
}

fn write_int(nbt: &mut Compound, key: &str, value: i32) {
    nbt.insert(JavaString::from(key), Tag::Int(value));
}
