// A right click puts an item on the altar or takes one off (API 21).
fn observe_right_click(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    face: [i32; 3],
    canceled: &mut bool,
) {
    crate::game::naturesaura_state::on_right_click(game, pos, face, canceled);
}
