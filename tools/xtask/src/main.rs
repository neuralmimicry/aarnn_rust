use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

fn usage() -> ! {
    eprintln!(
        "usage: cargo xtask build --product android --target <rust-target> \\\n+         --abi <android-abi> --out-dir <generated-jni-directory>"
    );
    exit(2);
}

fn option(args: &[String], name: &str) -> String {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .unwrap_or_else(|| {
            eprintln!("missing required option {name}");
            usage()
        })
}

fn android_ndk() -> PathBuf {
    if let Some(path) = env::var_os("ANDROID_NDK_HOME") {
        return PathBuf::from(path);
    }
    let sdk = env::var_os("ANDROID_SDK_ROOT")
        .or_else(|| env::var_os("ANDROID_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("HOME").unwrap_or_default()).join("Android/Sdk")
        });
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
        .unwrap_or_else(|| panic!("no Android NDK is installed under {}", ndk_root.display()))
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("build") {
        usage();
    }
    let product = option(&args, "--product");
    if product != "android" {
        eprintln!("unsupported product {product:?}; only android is available");
        exit(2);
    }
    let target = option(&args, "--target");
    let abi = option(&args, "--abi");
    let out_dir = PathBuf::from(option(&args, "--out-dir"));
    let ndk = android_ndk();
    let llvm = ndk.join("toolchains/llvm/prebuilt/linux-x86_64/bin");
    let sdk_api = env::var("ANDROID_API_LEVEL").unwrap_or_else(|_| "34".to_owned());
    let linker = llvm.join(format!("{target}{sdk_api}-clang"));
    let cxx = llvm.join(format!("{target}{sdk_api}-clang++"));
    if !linker.is_file() {
        panic!("Android linker is missing: {}", linker.display());
    }
    if !cxx.is_file() {
        panic!("Android C++ compiler is missing: {}", cxx.display());
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask must live at repository/tools/xtask")
        .to_path_buf();
    let target_env = format!(
        "CARGO_TARGET_{}_LINKER",
        target.to_ascii_uppercase().replace('-', "_")
    );
    let cc_env = format!("CC_{}", target.replace('-', "_"));
    let cxx_env = format!("CXX_{}", target.replace('-', "_"));
    let status = Command::new("cargo")
        .current_dir(&root)
        .args([
            "build",
            "--locked",
            "--manifest-path",
            "Cargo.toml",
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
        .expect("failed to start cargo Android build");
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }

    let source = root.join(format!("target/{target}/debug/libaarnn_rust.so"));
    if !source.is_file() {
        panic!("Cargo completed without producing {}", source.display());
    }
    fs::create_dir_all(&out_dir)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", out_dir.display()));
    fs::copy(&source, out_dir.join("libaarnn_rust.so"))
        .unwrap_or_else(|error| panic!("cannot copy {}: {error}", source.display()));
    println!("Android {abi} library written to {}", out_dir.display());
}
