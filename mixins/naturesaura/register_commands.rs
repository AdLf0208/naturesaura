// Declare `/naturesaura aura|spot|altar|altar-build|drain` (API 8).
fn observe_register(commands: &mut Vec<NativeCommand>) {
    crate::game::naturesaura_state::on_register_commands(commands);
}
