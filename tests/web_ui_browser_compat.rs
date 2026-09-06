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
fn shipped_html_management_client_has_a_gateway_route() {
    let html = read_asset("web_ui/index.html");
    let client = read_asset("web_ui/management-client.generated.js");
    let app = read_asset("web_ui/app.js");
    assert!(
        html.contains("/management-client.generated.js"),
        "the HTML shell must load the generated management client"
    );
    assert!(
        client.contains("AARNNGeneratedManagementClient"),
        "the generated management client must be shipped"
    );
    assert!(
        client.contains("/api/management/status"),
        "generated status must use the persisted management gateway"
    );
    assert!(
        client.contains("/api/management/operations") && client.contains("/api/operations/"),
        "generated operation calls must use the secured management gateway"
    );
    assert!(
        client.contains("cancelMigration") && client.contains("/api/management/migrations/cancel"),
        "generated migration client must expose fenced cancellation"
    );
    assert!(
        app.contains("submitManagedWorkspaceOperation")
            && app.contains("management.status")
            && app.contains("management.submitOperation"),
        "workspace control must use the generated management operation flow"
    );
    let gateway = read_asset("src/bin/web_ui.rs");
    assert!(
        gateway.contains("/management-client.generated.js")
            && gateway.contains("management_client_js"),
        "the Rust gateway must serve the generated management client"
    );
}

#[test]
fn placement_surface_is_shipped_for_web_and_native_clients() {
    let html = read_asset("web_ui/index.html");
    let app = read_asset("web_ui/app.js");
    let css = read_asset("web_ui/style.css");
    assert!(html.contains("data-surface-tab=\"placement\""));
    assert!(html.contains("placement-canvas"));
    assert!(app.contains("buildPlacementModel") && app.contains("renderPlacement"));
    assert!(app.contains("orchestrator report") && app.contains("workspace projection"));
    assert!(app.contains("shard_movements") && app.contains("normalizePlacementMovement"));
    assert!(app.contains("selectedShardIds") && app.contains("selectedLayers"));
    assert!(app.contains("ctrlKey || event.metaKey") && app.contains("dblclick"));
    assert!(app.contains("placementPointerToWorld") && app.contains("state.placement.camera"));
    assert!(app.contains("zoomPlacementToShard") && app.contains("placementDetailEl"));
    assert!(app.contains("state.placement.selectedLayers.has"));
    assert!(css.contains(".placement-surface") && css.contains(".surface-tab"));
    assert!(css.contains("touch-action: none") && css.contains(".placement-state.moving"));
    let native = read_asset("src/ui.rs");
    assert!(native.contains("placement_explorer"));
    assert!(native.contains("render_placement_explorer"));
    assert!(
        native.contains("placement_selected_shards")
            && native.contains("placement_selected_layers")
    );
    assert!(native.contains("double_clicked") && native.contains("placement_camera_rotation"));
    assert!(native.contains("shard_movements") && native.contains("Backup"));
    assert!(
        native.contains("Selected placement shard") && native.contains("neuron_count"),
        "native selection and detail surfaces must report the computed neuron count"
    );
}

#[test]
fn network_canvas_disables_default_touch_gestures() {
    let css = read_asset("web_ui/style.css");
    assert!(
        css.contains("#network-canvas") && css.contains("touch-action: none;"),
        "web_ui/style.css should disable default touch gestures on the network canvas"
    );
}

#[test]
fn browser_aer_adapter_is_bounded_and_has_no_global_hid_claim() {
    let source = read_asset("web_ui/aer-transport.js");
    assert!(source.contains("AARNNBrowserAerSession"));
    assert!(source.contains("MAX_PAYLOAD"));
    assert!(source.contains("migratePath"));
    assert!(!source.contains("KeyboardEvent") && !source.contains("pointerlock"));
}

#[test]
fn browser_aer_crc_uses_the_shared_binary_wire_layout() {
    let source = read_asset("web_ui/aer-transport.js");
    assert!(source.contains("canonicalBytes"));
    assert!(source.contains("setBigUint64"));
    assert!(source.contains("setUint32(offset, payload.length"));
    assert!(!source.contains("TextEncoder().encode(canonical"));
}
