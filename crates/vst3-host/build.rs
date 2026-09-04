use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(mixlink_vst3_sdk)");
    println!("cargo:rerun-if-changed=native/MixLinkVST3Bridge.mm");
    println!("cargo:rerun-if-changed=native/MixLinkVST3Bridge.h");
    println!("cargo:rerun-if-changed=native/MixLinkVST3Stub.c");
    println!("cargo:rerun-if-env-changed=MIXLINK_VST3_ARCHS");

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.parent().unwrap().parent().unwrap();
    let sdk_lib = root.join("build/vst3sdk/libvst3sdk.a");
    let script = root.join("Scripts/build-vst3-sdk.sh");

    let have_sdk = sdk_lib.exists();
    if !have_sdk && script.exists() && env::var("MIXLINK_BUILD_VST3").is_ok() {
        let status = Command::new("bash")
            .arg(&script)
            .env("MIXLINK_VST3_ARCHS", env::var("MIXLINK_VST3_ARCHS").unwrap_or_else(|_| "arm64".into()))
            .current_dir(root)
            .status();
        if let Ok(s) = status {
            if !s.success() {
                println!("cargo:warning=VST3 SDK build failed; using stub host");
            }
        }
    }

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    if sdk_lib.exists() {
        let sdk_inc = root.join("Vendor/vst3sdk");
        let status = Command::new("clang++")
            .args([
                "-c",
                "-fobjc-arc",
                "-std=c++17",
                "-fPIC",
                "-O2",
                "-o",
            ])
            .arg(out.join("MixLinkVST3Bridge.o"))
            .arg(manifest.join("native/MixLinkVST3Bridge.mm"))
            .arg(format!("-I{}", sdk_inc.display()))
            .arg(format!("-I{}", manifest.join("native").display()))
            .status()
            .expect("clang++");
        if !status.success() {
            panic!("failed to compile MixLinkVST3Bridge.mm");
        }
        Command::new("ar")
            .args(["crus"])
            .arg(out.join("libmixlinkvst3.a"))
            .arg(out.join("MixLinkVST3Bridge.o"))
            .status()
            .expect("ar");
        println!("cargo:rustc-link-search=native={}", out.display());
        println!("cargo:rustc-link-search=native={}", root.join("build/vst3sdk").display());
        println!("cargo:rustc-link-lib=static=mixlinkvst3");
        println!("cargo:rustc-link-lib=static=vst3sdk");
        println!("cargo:rustc-link-lib=framework=Cocoa");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=c++");
        println!("cargo:rustc-cfg=mixlink_vst3_sdk");
    } else {
        let status = Command::new("clang")
            .args(["-c", "-fPIC", "-O2", "-o"])
            .arg(out.join("MixLinkVST3Stub.o"))
            .arg(manifest.join("native/MixLinkVST3Stub.c"))
            .status()
            .expect("clang");
        if !status.success() {
            panic!("failed to compile VST3 stub");
        }
        Command::new("ar")
            .args(["crus"])
            .arg(out.join("libmixlinkvst3.a"))
            .arg(out.join("MixLinkVST3Stub.o"))
            .status()
            .expect("ar");
        println!("cargo:rustc-link-search=native={}", out.display());
        println!("cargo:rustc-link-lib=static=mixlinkvst3");
        println!("cargo:warning=VST3 SDK not present; MixLinkVST3* is a silent stub");
    }
}
