fn main() {
    println!("cargo:rerun-if-env-changed=AARNN_MUJOCO_DIR");
    #[cfg(feature = "native")]
    {
        let sdk = std::path::PathBuf::from(
            std::env::var_os("AARNN_MUJOCO_DIR")
                .expect("native adapter requires AARNN_MUJOCO_DIR pointing to MuJoCo 3.17.0"),
        );
        let header = sdk.join("include/mujoco/mujoco.h");
        println!("cargo:rerun-if-changed={}", header.display());
        assert!(
            std::fs::read_to_string(&header)
                .expect("MuJoCo header")
                .contains("#define mjVERSION_HEADER 3017000"),
            "MuJoCo headers must be exactly 3.17.0"
        );
        let bindings=bindgen::Builder::default().header(header.to_string_lossy())
            .clang_arg(format!("-I{}",sdk.join("include").display()))
            .allowlist_function("mj_(loadXML|makeData|deleteData|deleteModel|step|forward|resetData|name2id|id2name|stateSize|getState|setState|applyFT|objectVelocity|objectAcceleration|rnePostConstraint|contactForce|ray|version|versionString)")
            .allowlist_type("mjModel|mjData|mjtObj|mjtState|mjtJoint")
            .allowlist_function("mjv_(defaultCamera|defaultOption|defaultScene|makeScene|freeScene|updateScene|moveCamera)")
            .allowlist_function("mjr_(defaultContext|makeContext|freeContext|render|readPixels)")
            .allowlist_type("mjvCamera|mjvOption|mjvScene|mjrContext|mjrRect|mjtCatBit|mjtMouse|mjtFontScale")
            .derive_default(false).layout_tests(false).generate().expect("generate MuJoCo 3.17 bindings");
        bindings
            .write_to_file(
                std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("mujoco.rs"),
            )
            .unwrap();
        println!(
            "cargo:rustc-link-search=native={}",
            sdk.join("lib").display()
        );
        println!("cargo:rustc-link-lib=dylib=mujoco");
    }
}
