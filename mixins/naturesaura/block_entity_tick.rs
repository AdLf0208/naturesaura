// One server tick: the bloom's generation, the altar's ritual (API 17).
fn observe_block_entity_tick(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &mut horizon_client::containers::ModBlockEntity,
) {
    crate::game::naturesaura_state::on_tick(game, pos, entity);
}
