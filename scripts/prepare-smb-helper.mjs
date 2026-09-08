/**
 * Build `ams-smb-helper` and copy to `src-tauri/binaries/` with target-triple
 * suffix for Tauri `externalBin` (Phase 20d).
 *
 * Usage:
 *   node scripts/prepare-smb-helper.mjs
 *   node scripts/prepare-smb-helper.mjs --release
 */
import {existsSync, mkdirSync, copyFileSync, chmodSync, statSync} from "node:fs";
import {join, dirname} from "node:path";
import {fileURLToPath} from "node:url";
import {execFileSync} from "node:child_process";
import {platform} from "node:os";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");
const tauriDir = join(root, "src-tauri");
const binariesDir = join(tauriDir, "binaries");
const release = process.argv.includes("--release");
const profile = release ? "release" : "debug";
const ext = platform() === "win32" ? ".exe" : "";
const cargoArgs = [
  "build",
  "--manifest-path",
  join(tauriDir, "Cargo.toml"),
  "--bin",
  "ams-smb-helper",
];
if (release) cargoArgs.push("--release");

function targetTriple() {
  const out = execFileSync("rustc", ["-vV"], {encoding: "utf8"});
  const m = /host: (\S+)/.exec(out);
  if (!m) throw new Error("Could not detect rustc host triple");
  return m[1];
}

/** Resolve Cargo target dir (respects CARGO_TARGET_DIR / shared-target). */
function cargoTargetDir() {
  const fromEnv = process.env.CARGO_TARGET_DIR?.trim();
  if (fromEnv) return fromEnv;
  try {
    const out = execFileSync(
      "cargo",
      [
        "metadata",
        "--format-version",
        "1",
        "--no-deps",
        "--manifest-path",
        join(tauriDir, "Cargo.toml"),
      ],
      {encoding: "utf8", cwd: root, shell: platform() === "win32"},
    );
    const meta = JSON.parse(out);
    if (meta.target_directory) return meta.target_directory;
  } catch {
    // fall through
  }
  return join(tauriDir, "target");
}

function main() {
  console.log(`> cargo ${cargoArgs.join(" ")}`);
  execFileSync("cargo", cargoArgs, {
    cwd: root,
    stdio: "inherit",
    shell: platform() === "win32",
  });

  const triple = targetTriple();
  const targetDir = cargoTargetDir();
  const built = join(targetDir, profile, `ams-smb-helper${ext}`);
  if (!existsSync(built) || statSync(built).size === 0) {
    throw new Error(`Helper binary missing or empty: ${built}`);
  }

  mkdirSync(binariesDir, {recursive: true});
  const dest = join(binariesDir, `ams-smb-helper-${triple}${ext}`);
  copyFileSync(built, dest);
  if (platform() !== "win32") {
    chmodSync(dest, 0o755);
  }
  console.log(`Copied ${built} -> ${dest} (${statSync(dest).size} bytes)`);
}

main();
