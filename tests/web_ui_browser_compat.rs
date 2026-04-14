use std::fs;
use std::path::PathBuf;

fn asset_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_asset(relative: &str) -> String {
    fs::read_to_string(asset_path(relative))
        .unwrap_or_else(|err| panic!("failed to read asset {relative}: {err}"))
}

#[test]
fn shipped_web_ui_scripts_avoid_mobile_hostile_optional_syntax() {
    for relative in ["web_ui/app.js", "web_ui/shell.js"] {
        let source = read_asset(relative);
        assert!(
            !source.contains("?."),
            "{relative} still contains optional chaining syntax"
        );
        assert!(
            !source.contains("??"),
            "{relative} still contains nullish coalescing syntax"
        );
    }
}

#[test]
fn network_canvas_disables_default_touch_gestures() {
    let css = read_asset("web_ui/style.css");
    assert!(
        css.contains("#network-canvas") && css.contains("touch-action: none;"),
        "web_ui/style.css should disable default touch gestures on the network canvas"
    );
}
