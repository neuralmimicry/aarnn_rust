fn locked_package_version(lockfile: &str, package_name: &str) -> Option<String> {
    let mut current_package_matches = false;
    for line in lockfile.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            current_package_matches = false;
        } else if line == format!("name = \"{package_name}\"") {
            current_package_matches = true;
        } else if current_package_matches
            && let Some(version) = line
                .strip_prefix("version = \"")
                .and_then(|version| version.strip_suffix('"'))
        {
            return Some(version.to_string());
        }
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=Cargo.lock");
    let lockfile = std::fs::read_to_string("Cargo.lock")?;
    let wgpu_version =
        locked_package_version(&lockfile, "wgpu").unwrap_or_else(|| "unavailable".to_string());
    println!("cargo:rustc-env=AARNN_WGPU_API_VERSION={wgpu_version}");

    println!("cargo:rerun-if-changed=proto/distributed.proto");
    println!("cargo:rerun-if-changed=proto/management.proto");
    println!("cargo:rerun-if-changed=proto");
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &["proto/distributed.proto", "proto/management.proto"],
            &["proto"],
        )?;
    Ok(())
}
