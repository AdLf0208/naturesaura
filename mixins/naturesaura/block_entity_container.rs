// The altar's one slot (API 16/17).
fn observe_block_entity_container(
    block: &str,
    container: &mut Option<horizon_client::containers::ModContainer>,
) {
    crate::game::naturesaura_state::on_container(block, container);
}
