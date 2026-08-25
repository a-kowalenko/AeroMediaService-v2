/**
 * Interactive SemVer release: bump → changelog → commit → tag → push.
 *
 * Usage: npm run release
 * Asks: patch | minor | major, then confirmation.
 *
 * Requires meaningful notes under ## [Unreleased] in CHANGELOG.md.
 * CI publishes that section as the public GitHub release body (updater notes).
 */
import { createInterface } from "node:readline/promises";
import { stdin as input, stdout as output } from "node:process";
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import {
  CHANGELOG_PATH,
  insertVersionNotes,
  readChangelog,
  resolveNotesForRelease,
  writeChangelog,
} from "./changelog.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");

const FILES = {
  packageJson: join(root, "package.json"),
  packageLock: join(root, "package-lock.json"),
  cargoToml: join(root, "src-tauri", "Cargo.toml"),
  cargoLock: join(root, "src-tauri", "Cargo.lock"),
  tauriConf: join(root, "src-tauri", "tauri.conf.json"),
  changelog: CHANGELOG_PATH,
};

function git(args, opts = {}) {
  const stdio = opts.stdio ?? ["ignore", "pipe", "pipe"];
  const out = execFileSync("git", args, {
    cwd: root,
    encoding: "utf8",
    ...opts,
    stdio,
  });
  // With stdio: "inherit", Node returns null (no captured stdout).
  if (out == null) return "";
  return String(out).trim();
}

function parseSemver(v) {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(v);
  if (!m) throw new Error(`Ungültige Version: ${v}`);
  return { major: Number(m[1]), minor: Number(m[2]), patch: Number(m[3]) };
}

function bump(version, kind) {
  const s = parseSemver(version);
  if (kind === "major") return `${s.major + 1}.0.0`;
  if (kind === "minor") return `${s.major}.${s.minor + 1}.0`;
  if (kind === "patch") return `${s.major}.${s.minor}.${s.patch + 1}`;
  throw new Error(`Unbekannter Bump-Typ: ${kind}`);
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function writeJson(path, data) {
  writeFileSync(path, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function setCargoTomlVersion(next) {
  let text = readFileSync(FILES.cargoToml, "utf8");
  // Only the package table at the top — first bare `version =` after [package]
  const replaced = text.replace(
    /(\[package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m,
    `$1${next}$3`,
  );
  if (replaced === text) {
    throw new Error("Konnte version in Cargo.toml nicht setzen");
  }
  writeFileSync(FILES.cargoToml, replaced, "utf8");
}

function setCargoLockPackageVersion(next) {
  let text = readFileSync(FILES.cargoLock, "utf8");
  const replaced = text.replace(
    /(\[\[package\]\]\nname = "aero-media-service"\nversion = ")([^"]+)(")/,
    `$1${next}$3`,
  );
  if (replaced !== text) {
    writeFileSync(FILES.cargoLock, replaced, "utf8");
  }
}

function setPackageLockRootVersion(next) {
  const lock = readJson(FILES.packageLock);
  lock.version = next;
  if (lock.packages?.[""]) {
    lock.packages[""].version = next;
  }
  writeJson(FILES.packageLock, lock);
}

function applyVersions(next) {
  const pkg = readJson(FILES.packageJson);
  pkg.version = next;
  writeJson(FILES.packageJson, pkg);

  setPackageLockRootVersion(next);

  const tauri = readJson(FILES.tauriConf);
  tauri.version = next;
  writeJson(FILES.tauriConf, tauri);

  setCargoTomlVersion(next);
  setCargoLockPackageVersion(next);
}

function assertReadyToRelease() {
  const branch = git(["branch", "--show-current"]);
  if (branch !== "master" && branch !== "main") {
    throw new Error(`Release nur von master/main (aktuell: ${branch})`);
  }

  const status = git(["status", "--porcelain"]);
  if (status) {
    throw new Error(
      "Working tree ist nicht sauber. Bitte zuerst alle Änderungen committen, dann erneut Release starten.\n\n" +
        status,
    );
  }

  git(["fetch", "origin", branch], { stdio: "inherit" });
  const local = git(["rev-parse", "HEAD"]);
  const remote = git(["rev-parse", `origin/${branch}`]);
  if (local !== remote) {
    throw new Error(
      `Branch ${branch} ist nicht synchron mit origin/${branch}. Bitte pull/push, dann erneut versuchen.`,
    );
  }
}

async function ask(rl, question) {
  return (await rl.question(question)).trim();
}

async function main() {
  // JetBrains Run-Konsole ist oft kein echtes TTY — ohne terminal:false
  // erscheinen Cursor-Escape-Sequenzen (z. B. ←[1G←[0J) im Output.
  const rl = createInterface({
    input,
    output,
    terminal: false,
  });
  try {
    assertReadyToRelease();

    const current = readJson(FILES.packageJson).version;
    console.log(`\nAktuelle Version: ${current}\n`);
    console.log("Release-Typ wählen:");
    console.log("  1) patch  — Bugfixes        (x.y.Z)");
    console.log("  2) minor  — neue Features   (x.Y.0)");
    console.log("  3) major  — Breaking Change (X.0.0)");
    console.log("  q) abbrechen\n");

    const choice = (await ask(rl, "Auswahl [1/2/3/q]: ")).toLowerCase();
    const kindMap = {
      1: "patch",
      patch: "patch",
      2: "minor",
      minor: "minor",
      3: "major",
      major: "major",
    };
    if (choice === "q" || choice === "quit" || choice === "abort") {
      console.log("Abgebrochen.");
      return;
    }
    const kind = kindMap[choice];
    if (!kind) {
      throw new Error(`Ungültige Auswahl: ${choice}`);
    }

    const next = bump(current, kind);
    const tag = `v${next}`;
    console.log(`\n→ ${kind}: ${current} → ${next} (Tag ${tag})\n`);

    const changelog = readChangelog();
    const { body: notesBody, source, fromVersion } = resolveNotesForRelease(
      changelog,
      kind,
      current,
    );
    if (source === "previous") {
      console.log(
        `Release-Notes: [Unreleased] leer — übernommen von ${fromVersion}:\n`,
      );
    } else {
      console.log("Release-Notes aus [Unreleased]:\n");
    }
    console.log(notesBody);
    console.log("");

    const confirm = (
      await ask(rl, `Release ${tag} committen, taggen und nach origin pushen? [y/N]: `)
    ).toLowerCase();
    if (confirm !== "y" && confirm !== "yes" && confirm !== "j" && confirm !== "ja") {
      console.log("Abgebrochen.");
      return;
    }

    writeChangelog(insertVersionNotes(changelog, next, notesBody));
    applyVersions(next);

    git(
      [
        "add",
        "package.json",
        "package-lock.json",
        "src-tauri/Cargo.toml",
        "src-tauri/Cargo.lock",
        "src-tauri/tauri.conf.json",
        "CHANGELOG.md",
      ],
      { stdio: "inherit" },
    );
    git(["commit", "-m", `release: ${next}`], { stdio: "inherit" });
    git(["tag", tag], { stdio: "inherit" });

    const branch = git(["branch", "--show-current"]);
    console.log(`\nPush ${branch} + ${tag} …`);
    git(["push", "origin", branch], { stdio: "inherit" });
    git(["push", "origin", tag], { stdio: "inherit" });

    console.log(`\nFertig. Release-Workflow sollte für ${tag} starten.`);
    console.log(
      "Releases: https://github.com/a-kowalenko/aero-media-service-releases/releases\n",
    );
  } finally {
    rl.close();
  }
}

main().catch((err) => {
  console.error(`\nFehler: ${err.message || err}\n`);
  process.exit(1);
});
