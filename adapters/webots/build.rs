fn main() {
    println!("cargo:rerun-if-env-changed=WEBOTS_HOME");
    #[cfg(feature = "native")]
    {
        let path = std::path::PathBuf::from(
            std::env::var_os("WEBOTS_HOME").unwrap_or_else(|| "/usr/local/webots".into()),
        );
        assert!(
            path.join("include/controller/c/webots/robot.h").is_file(),
            "Webots C SDK required"
        );
        println!(
            "cargo:rustc-link-search=native={}",
            path.join("lib/controller").display()
        );
        println!("cargo:rustc-link-lib=dylib=Controller");
    }
}
