use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let shim_dir = manifest_dir.join("go-shim");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    // Determine backend build tags from Cargo features.
    // For the podman backend we add two extra tags to avoid pulling in C library
    // dependencies (libgpgme, libbtrfs, libdevmapper) that are not present on CI
    // runners and are not needed for the docker:// and oci: transports we use:
    //   containers_image_openpgp  — use Go's native OpenPGP instead of libgpgme
    //   exclude_graphdriver_btrfs — omit the btrfs storage driver (needs libbtrfs-dev)
    //   exclude_graphdriver_devicemapper — omit the device-mapper driver
    let go_tags = if env::var("CARGO_FEATURE_PODMAN").is_ok() {
        "podman,containers_image_openpgp,exclude_graphdriver_btrfs,exclude_graphdriver_devicemapper"
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

    let mut cmd = Command::new("go");
    cmd.current_dir(&shim_dir)
        .env("CGO_ENABLED", "1")
        .arg("build")
        .arg(format!("-tags={}", go_tags))
        .arg("-buildmode=c-archive")
        .arg("-o")
        .arg(&lib_path)
        .arg(".");

    // On *-pc-windows-msvc targets the Rust linker (lld-link, set via
    // RUSTFLAGS in CI) requires MSVC-ABI COFF objects from Go's CGO.
    //
    // clang in GCC-driver mode is the only C compiler that:
    //   • accepts all GCC-style flags Go passes (-Werror, -dM, -fno-stack-protector)
    //   • produces MSVC-compatible COFF when given the right --target
    //
    // We cannot rely on CGO_CFLAGS to pass --target because Go's CGO security
    // filter may strip unrecognised flags before they reach clang.  Instead we
    // write a tiny .cmd wrapper that hard-codes --target as part of the CC
    // command itself; this is unconditional and cannot be filtered out.
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
        let msvc_triple = match arch.as_str() {
            "x86_64"  => "x86_64-pc-windows-msvc",
            "aarch64" => "aarch64-pc-windows-msvc",
            other     => panic!("unsupported Windows MSVC arch: {}", other),
        };
        // The wrapper calls clang with a fixed --target then forwards all other
        // args (%*).  Go treats this .cmd as the C compiler.
        let wrapper = out_dir.join("cgo_cc.cmd");
        fs::write(
            &wrapper,
            format!("@echo off\r\nclang --target={} %*\r\n", msvc_triple),
        )
        .expect("write CGO CC wrapper");
        cmd.env("CC", &wrapper);
    }

    // Align the Go shim's minimum macOS version with Rust's aarch64-apple-darwin
    // deployment target (11.0).  Without this Go defaults to the SDK version
    // (15.x on macos-15 runners), producing objects that emit "built for newer
    // macOS" warnings and may reference symbols gated behind the newer version.
    if target_os == "macos" {
        cmd.env("MACOSX_DEPLOYMENT_TARGET", "11.0");
    }

    let status = cmd
        .status()
        .expect("Failed to invoke `go build` — is Go (1.22+) installed and on PATH?");

    if !status.success() {
        panic!("Go shim build failed for tags={}", go_tags);
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
            // The podman backend (and Go's CGO net resolver in general) references
            // res_9_ninit / res_9_nclose / res_9_nsearch from libresolv.
            println!("cargo:rustc-link-lib=resolv");
        }
        "windows" => {
            println!("cargo:rustc-link-lib=bcrypt");
            println!("cargo:rustc-link-lib=ws2_32");
            println!("cargo:rustc-link-lib=userenv");
            // With CC=cl the CGO objects are compiled by MSVC which links the CRT
            // automatically; legacy_stdio_definitions is not needed and causes
            // LNK4078 / LNK1223 when mixed with MSVC-format objects.
        }
        _ => {}
    }

    println!("cargo:rerun-if-changed=go-shim/");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_PODMAN");
}

