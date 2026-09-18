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
    assert!(app.contains("hierarchical_shards") && app.contains("subShardCount"));
    assert!(app.contains("area_label") && app.contains("areaLabel"));
    assert!(app.contains("area shards") && app.contains("sub-shards"));
    assert!(app.contains("orchestrator report") && app.contains("workspace projection"));
    assert!(app.contains("shard_movements") && app.contains("normalizePlacementMovement"));
    assert!(app.contains("selectedShardIds") && app.contains("selectedLayers"));
    assert!(app.contains("ctrlKey || event.metaKey") && app.contains("dblclick"));
    assert!(app.contains("placementPointerToWorld") && app.contains("state.placement.camera"));
    assert!(app.contains("zoomPlacementToShard") && app.contains("placementDetailEl"));
    assert!(app.contains("state.placement.selectedLayers.has"));
    assert!(
        app.contains("layerIndex >= 0 && layerIndex < hidden.length")
            && app.contains("layerIndex === hidden.length"),
        "placement activity must map distributed layer 0 to hidden activity and the final layer to output activity"
    );
    assert!(css.contains(".placement-surface") && css.contains(".surface-tab"));
    assert!(css.contains("touch-action: none") && css.contains(".placement-state.moving"));
    let native = read_asset("src/ui.rs");
    assert!(native.contains("placement_explorer"));
    assert!(native.contains("render_placement_explorer"));
    assert!(native.contains("render_hierarchical_placement_explorer"));
    assert!(native.contains("hierarchical_shards") && native.contains("sub_shard_ids"));
    assert!(native.contains("area_label") && native.contains("area_label.as_str()"));
    assert!(native.contains("Area shard") && native.contains("latency_to_group_anchor_us"));
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
fn distributed_activity_exposes_current_sensory_frame() {
    let distributed = read_asset("src/distributed.rs");
    assert!(
        distributed.contains("spk_hist_s") && distributed.contains("sensory: Some(sensory)"),
        "the activity RPC must expose the current sensory frame instead of an unconditional empty envelope"
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

#[test]
fn video_input_source_parity_is_present_across_ui_and_cli_surfaces() {
    let native = read_asset("src/ui.rs");
    let cli = read_asset("src/main.rs");
    let web = read_asset("web_ui/index.html");
    let web_app = read_asset("web_ui/app.js");
    let android =
        read_asset("apps/android/app/src/main/java/com/neuralmimicry/aarnn/MainActivity.kt");
    let android_capabilities =
        read_asset("apps/android/app/src/main/java/com/neuralmimicry/aarnn/AndroidCapabilities.kt");
    let ios = read_asset("apps/ios/AarnnVideoInputView.swift");

    for source in [&native, &cli, &web, &web_app, &android, &ios] {
        assert!(
            source.contains("video-file"),
            "video-file source missing from one interface"
        );
        assert!(
            source.contains("camera"),
            "camera source missing from one interface"
        );
    }
    assert!(native.contains("Pop Out Preview") && native.contains("render_video_preview"));
    assert!(native.contains("list_webcam_devices") && native.contains("webcam-device"));
    assert!(native.contains("new_with_device_id") && native.contains("CombinedVideoAudioProvider"));
    assert!(native.contains("Graphic EQ enabled"));
    assert!(
        native.contains("show_equalizer: startup_audio_loaded || startup_video_audio_active"),
        "CLI/startup video audio must enable the native Graphic EQ"
    );
    assert!(
        native
            .contains("sim_audio_diagnostic = startup_audio_loaded || startup_video_audio_active"),
        "startup video audio must use the same diagnostic path as audio-file input"
    );
    assert!(web.contains("io-video-popout") && web_app.contains("openVideoPopout"));
    assert!(web.contains("io-camera-device") && web.contains("io-audio-device"));
    assert!(web_app.contains("enumerateDevices") && web_app.contains("videoAudioEnabled"));
    assert!(web_app.contains("getUserMedia({ video, audio })"));
    assert!(android.contains("Pop out video") && android.contains("VideoPreviewDialog"));
    assert!(android.contains("Camera.open(cameraIndex)") && android.contains("cameraCount"));
    assert!(android.contains("Include microphone audio"));
    assert!(android_capabilities.contains("camera_preview"));
    assert!(
        android_capabilities.contains("\"video-file\"")
            && android_capabilities.contains("\"camera\"")
    );
    assert!(cli.contains("video_file") && cli.contains("camera"));
    assert!(ios.contains("DiscoverySession") && ios.contains("includeAudio"));
}

#[test]
fn webgl_simulator_is_shipped_through_the_authenticated_gateway() {
    let html = read_asset("web_ui/webgl-sim.html");
    let source = read_asset("web_ui/webgl-sim.js");
    let app = read_asset("web_ui/app.js");
    let gateway = read_asset("src/bin/web_ui.rs");
    let launcher = read_asset("scripts/run_sim.sh");
    assert!(html.contains("webgl-canvas") && html.contains("webgl-network"));
    assert!(read_asset("web_ui/index.html").contains("/sim/webgl"));
    assert!(source.contains("getContext(\"webgl\"") && source.contains("/api/aer/infer"));
    for profile in [
        "celegans",
        "drosophila_banc",
        "drosophila_fafb",
        "hexapod",
        "nao",
        "zebrafish",
    ] {
        let catalogue: serde_json::Value =
            serde_json::from_str(&read_asset("sim/content/compiled.generated.json")).unwrap();
        assert!(
            catalogue["profiles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == profile),
            "missing WebGL profile {profile}"
        );
    }
    assert!(source.contains("NmSimContent.profiles") && html.contains("/sim-content.generated.js"));
    assert!(gateway.contains("sim_content_js") && gateway.contains("webgl_world_js"));
    assert!(source.contains("AARNNBrowserAerSession"));
    assert!(
        app.contains("const RASTER_HISTORY = 240"),
        "browser raster window must match the native Rust UI window"
    );
    assert!(
        source.contains("source_sequence: frame"),
        "WebGL AER frames must provide the required source sequence"
    );
    assert!(gateway.contains("/aer/infer") && gateway.contains("webgl_sim_html"));
    assert!(gateway.contains("default_network") && gateway.contains("default_node"));
    assert!(source.contains("webgl-control-surface-link") && source.contains("/api/config"));
    assert!(launcher.contains("webgl") && launcher.contains("--nodes"));
    assert!(
        launcher.contains("robot_io") && launcher.contains("IpcUdsServer"),
        "WebGL launcher must build and validate the IPC-capable cluster binary"
    );
    assert!(!source.contains("KeyboardEvent") && !source.contains("pointerlock"));
}

#[test]
fn activity_polling_coalesces_and_rejects_stale_sources() {
    let app = read_asset("web_ui/app.js");
    assert!(app.contains("activityFetchInFlight"));
    assert!(app.contains("activityFetchQueued"));
    assert!(app.contains("const requestSeq = ++activityRequestSeq"));
    assert!(app.contains("requestSeq === activityRequestSeq"));
    assert!(app.contains("finally {\n    activityFetchInFlight = false"));
}

#[test]
fn native_remote_workspace_actions_are_backgrounded() {
    let native = read_asset("src/ui.rs");
    assert!(native.contains("queue_remote_workspace_push"));
    assert!(native.contains("queue_remote_workspace_pull"));
    assert!(native.contains("queue_remote_workspace_control"));
    assert!(native.contains("ToolTaskResult::RemoteWorkspacePush"));
    assert!(native.contains("ToolTaskResult::RemoteWorkspacePull"));
    assert!(native.contains("ToolTaskResult::RemoteWorkspaceControl"));
    assert!(native.contains("remote_workspace_action_inflight"));
    assert!(native.contains("std::thread::spawn(move ||"));
    assert!(!native.contains("self.pull_remote_workspace_snapshot()"));
    assert!(!native.contains("self.control_remote_workspace_backend("));
}

#[test]
fn android_refresh_is_off_main_thread_and_coalesced() {
    let controller = read_asset(
        "apps/android/app/src/main/java/com/neuralmimicry/aarnn/RemoteConnectionController.kt",
    );
    assert!(controller.contains("Executors.newSingleThreadExecutor"));
    assert!(controller.contains("AtomicBoolean"));
    assert!(controller.contains("refreshInFlight.compareAndSet(false, true)"));
    assert!(controller.contains("refreshInFlight.set(false)"));
}
