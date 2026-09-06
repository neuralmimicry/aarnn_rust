//! Repository QA/build orchestration.
//!
//! This binary intentionally stays dependency-light. It is the single place
//! where documented QA commands map to bounded Cargo/product lanes; platform
//! lanes unavailable on the host are reported as `not-run`, never as a pass.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask must live at repository/tools/xtask")
        .to_path_buf()
}

fn usage() -> ! {
    eprintln!(
        "usage: cargo xtask doctor [--product PRODUCT]\n\
         cargo xtask examples list|run --id ID [--product PRODUCT]\n\
         cargo xtask qa run --suite SUITE [--product PRODUCT]\n\
         cargo xtask qa matrix [--available] [--include-examples]\n\
         cargo xtask bindings check|fingerprint\n\
         cargo xtask build --product android --target TARGET --abi ABI --out-dir DIR"
    );
    std::process::exit(2)
}

fn bindings_check(root: &Path) -> bool {
    let schema = fs::read_to_string(root.join("proto/management.proto"));
    let outputs = [
        root.join("src/generated_management.rs"),
        root.join("web_ui/management-client.generated.js"),
        root.join(
            "apps/android/app/src/main/java/com/neuralmimicry/aarnn/ManagementClient.generated.kt",
        ),
    ];
    let Ok(schema) = schema else {
        eprintln!("[bindings] management schema is missing");
        return false;
    };
    let source_marker = format!(
        "management-schema-source-digest:{}",
        schema_source_digest(&schema)
    );
    let mut ok = ["GetStatus", "SubmitOperation", "GetOperation"]
        .iter()
        .all(|method| schema.contains(method));
    for output in outputs {
        match fs::read_to_string(&output) {
            Ok(text)
                if text.contains("Generated")
                    && text.contains("SCHEMA_VERSION")
                    && text.contains(&source_marker) => {}
            Ok(_) => {
                eprintln!(
                    "[bindings] generated marker/schema/freshness digest missing from {} (expected {})",
                    output.display(),
                    source_marker
                );
                ok = false;
            }
            Err(error) => {
                eprintln!("[bindings] cannot read {}: {error}", output.display());
                ok = false;
            }
        }
    }
    let browser_app = fs::read_to_string(root.join("web_ui/app.js"));
    let android_client = fs::read_to_string(
        root.join("apps/android/app/src/main/java/com/neuralmimicry/aarnn/RemoteAarnnClient.kt"),
    );
    if !browser_app
        .as_deref()
        .is_ok_and(|source| source.contains("generatedManagementClient()"))
    {
        eprintln!("[bindings] browser application does not consume the generated client");
        ok = false;
    }
    if !android_client
        .as_deref()
        .is_ok_and(|source| source.contains("GeneratedManagementClient."))
    {
        eprintln!("[bindings] Android client does not consume generated management paths");
        ok = false;
    }
    if ok {
        println!(
            "{{\"management_schema\":1,\"rust\":\"build-generated\",\"browser\":true,\"android\":true}}"
        );
    }
    ok
}

fn schema_source_digest(schema: &str) -> String {
    // FNV-1a is used only as a cheap, stable stale-output detector. Protocol
    // integrity is provided by the schema/build toolchain, not this marker.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in schema.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn option(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn required_option(args: &[String], name: &str) -> String {
    option(args, name).unwrap_or_else(|| {
        eprintln!("missing required option {name}");
        usage()
    })
}

fn run(root: &Path, program: &str, args: &[&str]) -> bool {
    eprintln!("[qa] $ {program} {}", args.join(" "));
    Command::new(program)
        .current_dir(root)
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn doctor(root: &Path, product: &str) -> bool {
    let manifest_ok = root.join("Cargo.toml").is_file();
    let schema_ok = root.join("proto/management.proto").is_file()
        && root.join("proto/distributed.proto").is_file();
    let host_ok = manifest_ok && schema_ok && run(root, "cargo", &["check", "--locked", "--lib"]);
    let android = root.join("apps/android/gradlew").is_file();
    let ios = root.join("apps/ios").is_dir();
    println!(
        "{{\"product\":\"{}\",\"host\":{},\"management_schema\":{},\"android_project\":{},\"ios_project\":{},\"ios_status\":\"{}\"}}",
        product,
        host_ok,
        schema_ok,
        android,
        ios,
        if ios {
            "available"
        } else {
            "not-run: Xcode project absent"
        }
    );
    manifest_ok && schema_ok && host_ok
}

fn catalog_entries(root: &Path) -> Vec<(String, String)> {
    let text = fs::read_to_string(root.join("examples/catalog.toml")).unwrap_or_default();
    let mut entries = Vec::new();
    let mut id = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("id = \"") {
            id = value
                .strip_suffix('\"')
                .map(str::to_owned)
                .or_else(|| Some(value.to_owned()));
            continue;
        }
        if let (Some(id), Some(value)) = (id.take(), line.strip_prefix("title = \"")) {
            let title = value
                .strip_suffix('\"')
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_owned());
            entries.push((id, title));
        }
    }
    entries
}

fn examples_list(root: &Path) -> bool {
    for (id, title) in catalog_entries(root) {
        println!("{id}\t{title}");
    }
    true
}

fn example_test(id: &str) -> Option<&'static str> {
    match id {
        "EX-001" => Some("phase0_baseline"),
        "EX-002" | "EX-005" | "EX-007" => Some("phase2_to_phase8_gate"),
        "EX-011" => Some("mobile_contract"),
        "EX-012" => Some("failover_rejoin"),
        _ => None,
    }
}

fn examples_run(root: &Path, args: &[String]) -> bool {
    let id = required_option(args, "--id");
    if !scenario_manifest_is_complete(root, &id) {
        eprintln!("example {id} has no complete QA scenario manifest");
        return false;
    }
    let Some(test) = example_test(&id) else {
        eprintln!("unknown or externally gated example {id}; report is not-run");
        return false;
    };
    run(root, "cargo", &["test", "--locked", "--test", test])
}

fn scenario_manifest_is_complete(root: &Path, id: &str) -> bool {
    let path = root.join("qa/scenarios").join(format!("{id}.toml"));
    let Ok(manifest) = fs::read_to_string(path) else {
        return false;
    };
    [
        "id = ",
        "version = ",
        "title = ",
        "requirements = ",
        "products = ",
        "mode = ",
        "seed = ",
        "logical_stop = ",
        "oracle = ",
        "required_artifacts = ",
        "model_fixture_digest = ",
        "config_fixture_digest = ",
        "input_fixture_digest = ",
        "target_triples = ",
        "capabilities = ",
        "device_requirement = ",
        "resource_bounds = ",
        "latency_bounds = ",
        "reference_profile = ",
        "expected_digest_procedure = ",
        "admission_loss_policy = ",
    ]
    .iter()
    .all(|field| {
        manifest
            .lines()
            .any(|line| line.trim_start().starts_with(field))
    }) && manifest
        .lines()
        .any(|line| line.trim() == format!("id = \"{id}\""))
}

fn qa_suite(root: &Path, suite: &str) -> bool {
    match suite {
        "mobile-contract" => run(
            root,
            "cargo",
            &["test", "--locked", "--test", "mobile_contract"],
        ),
        "aer-transport" => run(
            root,
            "cargo",
            &["test", "--locked", "--lib", "aer_transport"],
        ),
        "federation" | "federation-discovery" => {
            run(root, "cargo", &["test", "--locked", "--lib", "federation"])
        }
        "browser-compat" => run(
            root,
            "cargo",
            &["test", "--locked", "--test", "web_ui_browser_compat"],
        ),
        "discovery-enrolment" => run(
            root,
            "cargo",
            &["test", "--locked", "--test", "mobile_contract"],
        ),
        "bindings" => bindings_check(root),
        "durability" | "migration" | "scientific-validation" | "consistent-cut" => {
            let module = match suite {
                "consistent-cut" => "consistent_cut",
                "scientific-validation" => "scientific_validation",
                other => other,
            };
            run(root, "cargo", &["test", "--locked", "--lib", module])
        }
        "recovery" => {
            let library = run(root, "cargo", &["test", "--locked", "--lib", "recovery"]);
            let process = run(
                root,
                "cargo",
                &["test", "--locked", "--test", "failover_rejoin"],
            );
            library && process
        }
        "section-21" | "all" => run(
            root,
            "cargo",
            &["test", "--locked", "--workspace", "--tests"],
        ),
        other => {
            eprintln!("unknown QA suite {other}; no test was silently skipped");
            false
        }
    }
}

fn qa_matrix(root: &Path, include_examples: bool) -> bool {
    let mut ok = doctor(root, "all-available");
    ok &= qa_suite(root, "section-21");
    ok &= qa_suite(root, "bindings");
    if include_examples {
        for (id, _) in catalog_entries(root) {
            ok &= examples_run(root, &["--id".to_owned(), id]);
        }
    }
    println!(
        "{{\"host\":{},\"android\":\"not-run-unless-explicitly-invoked\",\"ios\":\"not-run: Xcode project absent\"}}",
        ok
    );
    ok
}

fn android_ndk() -> PathBuf {
    if let Some(path) = env::var_os("ANDROID_NDK_HOME") {
        return PathBuf::from(path);
    }
    let sdk = env::var_os("ANDROID_SDK_ROOT")
        .or_else(|| env::var_os("ANDROID_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("Android/Sdk"));
    let ndk_root = sdk.join("ndk");
    let mut versions = fs::read_dir(&ndk_root)
        .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", ndk_root.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    versions.sort();
    versions
        .pop()
        .unwrap_or_else(|| panic!("no Android NDK under {}", ndk_root.display()))
}

fn android_build(root: &Path, args: &[String]) -> bool {
    if option(args, "--product").as_deref() != Some("android") {
        eprintln!("only the Android build lane is available");
        return false;
    }
    let target = required_option(args, "--target");
    let abi = required_option(args, "--abi");
    let out_dir = PathBuf::from(required_option(args, "--out-dir"));
    let ndk = android_ndk();
    let llvm = ndk.join("toolchains/llvm/prebuilt/linux-x86_64/bin");
    let sdk_api = env::var("ANDROID_API_LEVEL").unwrap_or_else(|_| "34".to_owned());
    let linker = llvm.join(format!("{target}{sdk_api}-clang"));
    let cxx = llvm.join(format!("{target}{sdk_api}-clang++"));
    if !linker.is_file() || !cxx.is_file() {
        eprintln!("Android toolchain is missing for target {target}");
        return false;
    }
    let target_env = format!(
        "CARGO_TARGET_{}_LINKER",
        target.to_ascii_uppercase().replace('-', "_")
    );
    let cc_env = format!("CC_{}", target.replace('-', "_"));
    let cxx_env = format!("CXX_{}", target.replace('-', "_"));
    let status = Command::new("cargo")
        .current_dir(root)
        .args([
            "build",
            "--locked",
            "--lib",
            "--target",
            &target,
            "--no-default-features",
            "--features",
            "mobile_android",
        ])
        .env(target_env, &linker)
        .env(cc_env, &linker)
        .env(cxx_env, &cxx)
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !status {
        return false;
    }
    let source = root.join(format!("target/{target}/debug/libaarnn_rust.so"));
    if !source.is_file() || fs::create_dir_all(&out_dir).is_err() {
        return false;
    }
    fs::copy(source, out_dir.join("libaarnn_rust.so")).is_ok() && {
        println!("Android {abi} library written to {}", out_dir.display());
        true
    }
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let root = root();
    let ok = match args.first().map(String::as_str) {
        Some("doctor") => doctor(
            &root,
            option(&args, "--product")
                .as_deref()
                .unwrap_or("all-available"),
        ),
        Some("examples") if args.get(1).map(String::as_str) == Some("list") => examples_list(&root),
        Some("examples") if args.get(1).map(String::as_str) == Some("run") => {
            examples_run(&root, &args[2..])
        }
        Some("qa") if args.get(1).map(String::as_str) == Some("run") => {
            qa_suite(&root, &required_option(&args, "--suite"))
        }
        Some("qa") if args.get(1).map(String::as_str) == Some("matrix") => {
            qa_matrix(&root, args.iter().any(|arg| arg == "--include-examples"))
        }
        Some("bindings") if args.get(1).map(String::as_str) == Some("check") => {
            bindings_check(&root)
        }
        Some("bindings") if args.get(1).map(String::as_str) == Some("fingerprint") => {
            match fs::read_to_string(root.join("proto/management.proto")) {
                Ok(schema) => {
                    println!("{}", schema_source_digest(&schema));
                    true
                }
                Err(error) => {
                    eprintln!("[bindings] cannot read management schema: {error}");
                    false
                }
            }
        }
        Some("build") => android_build(&root, &args),
        _ => usage(),
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_parser_discovers_every_catalogued_example() {
        let entries = catalog_entries(&root());
        assert_eq!(
            entries
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            vec!["EX-001", "EX-002", "EX-005", "EX-007", "EX-011", "EX-012"]
        );
        assert!(entries.iter().all(|(_, title)| !title.trim().is_empty()));
        assert!(
            entries
                .iter()
                .all(|(id, _)| scenario_manifest_is_complete(&root(), id))
        );
    }
}
