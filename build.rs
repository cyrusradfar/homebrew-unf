//! Build script for the UNFUDGED CLI binary.
//!
//! On macOS, embeds an Info.plist in the binary via a `__TEXT,__info_plist`
//! Mach-O section. This allows macOS Login Items to display "UNFUDGED"
//! instead of "Item from unidentified developer."

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os == "macos" {
        embed_info_plist();
    }
}

fn embed_info_plist() {
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let plist_path = out_dir.join("Info.plist");

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.unfudged.cli</string>
    <key>CFBundleName</key>
    <string>UNFUDGED</string>
    <key>CFBundleDisplayName</key>
    <string>UNFUDGED</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
</dict>
</plist>
"#
    );

    fs::write(&plist_path, plist_content).expect("Failed to write Info.plist");

    // Tell the linker to embed the plist as a Mach-O section
    println!("cargo:rustc-link-arg=-sectcreate");
    println!("cargo:rustc-link-arg=__TEXT");
    println!("cargo:rustc-link-arg=__info_plist");
    println!("cargo:rustc-link-arg={}", plist_path.display());

    // Rebuild if version changes
    println!("cargo:rerun-if-changed=Cargo.toml");
}
