use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Phase 20d: Tauri `externalBin` requires the triple-suffixed helper path to exist
    // at build-script time (`cargo test` / `tauri dev`). Release `beforeBuildCommand`
    // overwrites the placeholder with a real binary via prepare-smb-helper.mjs.
    ensure_smb_helper_bin_placeholder();
    tauri_build::build()
}

fn ensure_smb_helper_bin_placeholder() {
    let triple = env::var("TARGET")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(detect_host_triple)
        .unwrap_or_else(|| "x86_64-pc-windows-msvc".into());

    let exe = if triple.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let binaries = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()))
        .join("binaries");
    let dest = binaries.join(format!("ams-smb-helper-{triple}{exe}"));
    // Keep a real helper if prepare-smb-helper already wrote one; only stub when missing/empty.
    if dest.is_file() {
        if let Ok(meta) = fs::metadata(&dest) {
            if meta.len() > 0 {
                return;
            }
        }
    }
    let _ = fs::create_dir_all(&binaries);
    // Existence-only placeholder so tauri-build does not fail before prepare runs.
    let _ = fs::write(&dest, b"");
    println!("cargo:rerun-if-changed={}", dest.display());
    println!("cargo:rerun-if-changed={}", binaries.display());
}

fn detect_host_triple() -> Option<String> {
    let out = Command::new("rustc").arg("-vV").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
}
