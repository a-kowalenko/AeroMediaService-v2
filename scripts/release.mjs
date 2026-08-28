/**
 * Interactive SemVer release: bump × channel → changelog → commit → tag → push.
 *
 * Usage: npm run release
 *
 * Stable version:
 *   1) Choose bump: patch | minor | major
 *   2) Choose channel: stable | beta
 *
 * Already on prerelease (e.g. 0.1.14-beta.1):
 *   beta        → 0.1.14-beta.2
 *   stable      → 0.1.14 (promote)
 *   new beta    → bump core + x.y.z-beta.1 (patch | minor | major)
 *
 * Changelog:
 *   beta   → snapshot ## [x.y.z-beta.N] (Unreleased kept)
 *   stable → promote Unreleased → ## [x.y.z]
 */
import { createInterface } from "node:readline/promises";
import { stdin as input, stdout as output } from "node:process";
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import {
  CHANGELOG_PATH,
  insertBetaSnapshot,
  insertVersionNotes,
  readChangelog,
  resolveNotesForRelease,
  writeChangelog,
} from "./changelog.mjs";
import {
  bumpCore,
  isPrereleaseVersion,
  nextBetaLineVersion,
  nextBetaVersion,
  parseSemVer,
  toStableVersion,
} from "./semver.mjs";

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
  if (out == null) return "";
  return String(out).trim();
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function writeJson(path, data) {
  writeFileSync(path, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function setCargoTomlVersion(next) {
  let text = readFileSync(FILES.cargoToml, "utf8");
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

function assertTagAvailable(tag) {
  const local = git(["tag", "-l", tag]);
  if (local) {
    throw new Error(`Tag ${tag} existiert bereits lokal.`);
  }
  try {
    git(["ls-remote", "--exit-code", "--tags", "origin", `refs/tags/${tag}`]);
    throw new Error(`Tag ${tag} existiert bereits auf origin.`);
  } catch (e) {
    const msg = String(e?.message ?? e);
    if (msg.includes("existiert bereits")) throw e;
  }
}

async function ask(rl, question) {
  return (await rl.question(question)).trim();
}

const BUMP_KIND_MAP = {
  1: "patch",
  patch: "patch",
  2: "minor",
  minor: "minor",
  3: "major",
  major: "major",
};

/**
 * @returns {Promise<"major"|"minor"|"patch"|null>}
 */
async function askBumpKind(rl) {
  console.log("Ziel-Bump wählen:");
  console.log("  1) patch  — Bugfixes        (x.y.Z)");
  console.log("  2) minor  — neue Features   (x.Y.0)");
  console.log("  3) major  — Breaking Change (X.0.0)");
  console.log("  q) abbrechen\n");

  const bumpChoice = (await ask(rl, "Auswahl [1/2/3/q]: ")).toLowerCase();
  if (bumpChoice === "q" || bumpChoice === "quit" || bumpChoice === "abort") {
    return null;
  }
  const kind = BUMP_KIND_MAP[bumpChoice];
  if (!kind) {
    throw new Error(`Ungültige Auswahl: ${bumpChoice}`);
  }
  return kind;
}

/**
 * @returns {Promise<{ next: string, channel: "stable"|"beta", kind: "major"|"minor"|"patch"|"promote"|"beta-next" }|null>}
 */
async function resolveNextVersion(rl, current) {
  if (isPrereleaseVersion(current)) {
    console.log("Aktuell auf Vorabversion — nächster Schritt:");
    console.log("  1) beta            — nächste Beta        (…-beta.N+1)");
    console.log("  2) stable          — finale Version      (Suffix entfernen)");
    console.log("  3) neue Beta-Linie — Core bump + beta.1  (patch / minor / major)");
    console.log("  q) abbrechen\n");

    const choice = (await ask(rl, "Auswahl [1/2/3/q]: ")).toLowerCase();
    if (choice === "q" || choice === "quit" || choice === "abort") {
      return null;
    }
    if (choice === "1" || choice === "beta") {
      return {
        next: nextBetaVersion(current),
        channel: "beta",
        kind: "beta-next",
      };
    }
    if (choice === "2" || choice === "stable") {
      return {
        next: toStableVersion(current),
        channel: "stable",
        kind: "promote",
      };
    }
    if (
      choice === "3" ||
      choice === "neu" ||
      choice === "neue" ||
      choice === "line" ||
      choice === "bump"
    ) {
      console.log("");
      const kind = await askBumpKind(rl);
      if (!kind) {
        return null;
      }
      return {
        next: nextBetaLineVersion(current, kind),
        channel: "beta",
        kind,
      };
    }
    throw new Error(`Ungültige Auswahl: ${choice}`);
  }

  const kind = await askBumpKind(rl);
  if (!kind) {
    return null;
  }

  console.log("\nKanal wählen:");
  console.log("  1) stable — öffentlicher Release (wird nach CI Latest)");
  console.log("  2) beta   — Vorabversion (Prerelease, nicht Latest)");
  console.log("  q) abbrechen\n");

  const channelChoice = (await ask(rl, "Auswahl [1/2/q]: ")).toLowerCase();
  if (
    channelChoice === "q" ||
    channelChoice === "quit" ||
    channelChoice === "abort"
  ) {
    return null;
  }
  if (channelChoice === "1" || channelChoice === "stable") {
    return { next: bumpCore(current, kind), channel: "stable", kind };
  }
  if (channelChoice === "2" || channelChoice === "beta") {
    return { next: nextBetaVersion(current, kind), channel: "beta", kind };
  }
  throw new Error(`Ungültige Auswahl: ${channelChoice}`);
}

async function main() {
  const rl = createInterface({
    input,
    output,
    terminal: false,
  });
  try {
    assertReadyToRelease();

    const current = readJson(FILES.packageJson).version;
    parseSemVer(current);
    console.log(`\nAktuelle Version: ${current}\n`);

    const resolved = await resolveNextVersion(rl, current);
    if (!resolved) {
      console.log("Abgebrochen.");
      return;
    }

    const { next, channel, kind } = resolved;
    const tag = `v${next}`;
    const isBeta = channel === "beta";

    console.log(`\n→ ${current} → ${next} (Tag ${tag})`);
    if (isBeta) {
      console.log("  Kanal: beta (GitHub Prerelease, Latest bleibt unberührt)");
    } else {
      console.log("  Kanal: stable (CI setzt nach erfolgreichem Build automatisch Latest)");
    }
    console.log("");

    assertTagAvailable(tag);

    const notesKind =
      kind === "promote" || kind === "beta-next" ? "patch" : kind;
    const changelog = readChangelog();
    const { body: notesBody, source, fromVersion } = resolveNotesForRelease(
      changelog,
      notesKind,
      current,
      channel,
    );
    if (source === "previous") {
      console.log(
        `Release-Notes: [Unreleased] leer — übernommen von ${fromVersion}:\n`,
      );
    } else if (source === "stub") {
      console.log("Release-Notes: Stub (Unreleased leer):\n");
    } else {
      console.log(
        isBeta
          ? "Release-Notes-Snapshot aus [Unreleased] (Unreleased bleibt):\n"
          : "Release-Notes aus [Unreleased]:\n",
      );
    }
    console.log(notesBody);
    console.log("");

    const confirm = (
      await ask(
        rl,
        `Release ${tag} committen, taggen und nach origin pushen? [y/N]: `,
      )
    ).toLowerCase();
    if (
      confirm !== "y" &&
      confirm !== "yes" &&
      confirm !== "j" &&
      confirm !== "ja"
    ) {
      console.log("Abgebrochen.");
      return;
    }

    if (isBeta) {
      writeChangelog(insertBetaSnapshot(changelog, next, notesBody));
    } else {
      writeChangelog(insertVersionNotes(changelog, next, notesBody));
    }
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
    if (isBeta) {
      console.log("Beta: erscheint als Prerelease; Latest bleibt die aktuelle Stable.");
    } else {
      console.log("Stable: nach grünem CI-Build wird Latest automatisch gesetzt.");
    }
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
