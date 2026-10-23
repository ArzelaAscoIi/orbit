//! Cmd+V reads the system clipboard only with eframe's `clipboard`
//! feature. Orbit turns eframe's default features off, and 0.37 moved
//! clipboard access out of them: without the feature egui pastes the last
//! text copied inside Orbit, whatever is on the clipboard (v0.0.9).

#[test]
fn eframe_keeps_its_clipboard_feature() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("Cargo.toml");
    let eframe = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("eframe"))
        .expect("eframe in Cargo.toml");
    let default_off = eframe.contains("default-features = false");
    let features = eframe.split("features = [").nth(1).unwrap_or("");
    assert!(
        !default_off || features.contains("\"clipboard\""),
        "eframe needs the \"clipboard\" feature, or Cmd+V ignores the system clipboard: {eframe}"
    );
}
