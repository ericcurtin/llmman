use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let shim_dir = manifest_dir.join("go-shim");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    // Determine backend from Cargo features
    let backend = if env::var("CARGO_FEATURE_PODMAN").is_ok() {
        "podman"
    } else {
        "docker"
    };

    // Always build natively — each platform's CI runner builds its own binary.
    // CGO requires the host C toolchain, which is always present on native runners.
    let lib_name = if target_os == "windows" {
        "llmman_shim.lib"
    } else {
        "libllmman_shim.a"
    };
    let lib_path = out_dir.join(lib_name);

    // Ensure Go module dependencies are present
    let _ = Command::new("go")
        .current_dir(&shim_dir)
        .args(["mod", "download"])
        .status();

    let status = Command::new("go")
        .current_dir(&shim_dir)
        .env("CGO_ENABLED", "1")
        .arg("build")
        .arg(format!("-tags={}", backend))
        .arg("-buildmode=c-archive")
        .arg("-o")
        .arg(&lib_path)
        .arg(".")
        .status()
        .expect("Failed to invoke `go build` — is Go (1.22+) installed and on PATH?");

    if !status.success() {
        panic!("Go shim build failed for backend={}", backend);
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=llmman_shim");

    // Platform-specific link dependencies required by Go runtime and shim libraries
    match target_os.as_str() {
        "linux" => {
            println!("cargo:rustc-link-lib=pthread");
            println!("cargo:rustc-link-lib=dl");
        }
        "macos" => {
            println!("cargo:rustc-link-lib=framework=CoreFoundation");
            println!("cargo:rustc-link-lib=framework=Security");
            println!("cargo:rustc-link-lib=framework=SystemConfiguration");
        }
        "windows" => {
            println!("cargo:rustc-link-lib=bcrypt");
            println!("cargo:rustc-link-lib=ws2_32");
            println!("cargo:rustc-link-lib=userenv");
        }
        _ => {}
    }

    println!("cargo:rerun-if-changed=go-shim/");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_PODMAN");
}

