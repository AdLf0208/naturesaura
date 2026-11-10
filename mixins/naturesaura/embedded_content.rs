// Nature's Aura is one file on the registry: its blocks, items, models, textures, lang, loot,
// recipes and block tags are the content mod in this bundle's data/ (content/naturesaura.hmod,
// built by tools/assemble_content.py and copied in by tools/package.py's check). It is written
// out once and loaded as a dev mod is, so it is on whenever this native mod is built in. An
// installed copy of the mod wins, as the first read.
fn naturesaura_embedded_content(
    root: &std::path::Path,
    enabled: &std::collections::BTreeSet<String>,
    dev: &[std::path::PathBuf],
) -> core::ops::ControlFlow<std::path::PathBuf> {
    static CONTENT: &[u8] =
        horizon_client::native_data!("naturesaura", "naturesaura-content.hmod");
    let dir = std::env::current_dir()
        .unwrap_or_default()
        .join(".horizon-cache");
    let file = dir.join("naturesaura-embedded-content.hmod");
    if dev.contains(&file) {
        return core::ops::ControlFlow::Continue(());
    }
    let same = std::fs::read(&file).is_ok_and(|on_disk| on_disk == CONTENT);
    if !same
        && std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&file, CONTENT))
            .is_err()
    {
        log::warn!(
            "Horizon: Nature's Aura's content could not be written to {}",
            file.display()
        );
        return core::ops::ControlFlow::Continue(());
    }
    let mut with = dev.to_vec();
    with.push(file);
    core::ops::ControlFlow::Break(content_root(root, enabled, &with))
}
