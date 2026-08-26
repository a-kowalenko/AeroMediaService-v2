/**
 * CHANGELOG.md helpers for release.mjs and CI.
 *
 * Sections: ## [Unreleased] | ## [x.y.z] | ## [x.y.z-beta.N] - YYYY-MM-DD
 *
 * Beta: snapshot copy of [Unreleased] (Unreleased stays intact).
 * Stable: promote [Unreleased] into ## [x.y.z] (Unreleased cleared).
 */
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isPrereleaseVersion, parseSemVerLoose } from "./semver.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
export const CHANGELOG_PATH = join(__dirname, "..", "CHANGELOG.md");

const SECTION_RE = /^## \[([^\]]+)\](?:\s*-\s*(\d{4}-\d{2}-\d{2}))?\s*$/gm;

/**
 * @param {string} markdown
 * @returns {{ version: string, date: string | null, start: number, bodyStart: number, end: number }[]}
 */
export function parseSections(markdown) {
  const matches = [...markdown.matchAll(SECTION_RE)];
  const sections = [];
  for (let i = 0; i < matches.length; i++) {
    const m = matches[i];
    const start = m.index ?? 0;
    const headerEnd = start + m[0].length;
    const end = i + 1 < matches.length ? (matches[i + 1].index ?? markdown.length) : markdown.length;
    sections.push({
      version: m[1],
      date: m[2] ?? null,
      start,
      bodyStart: headerEnd,
      end,
    });
  }
  return sections;
}

/**
 * Body of a version section (without the ## heading), trimmed.
 * @param {string} markdown
 * @param {string} version  e.g. "0.3.0", "0.3.0-beta.1", or "Unreleased"
 */
export function extractSectionBody(markdown, version) {
  const sections = parseSections(markdown);
  const section = sections.find((s) => s.version === version);
  if (!section) return null;
  return markdown.slice(section.bodyStart, section.end).trim();
}

/**
 * Notes shown on GitHub / in the updater for tag vX.Y.Z / vX.Y.Z-beta.N.
 * Prefers ## [version]; for missing stable patch sections walks back.
 * Prerelease versions never walk back to an older stable section.
 */
export function releaseNotesForVersion(markdown, version) {
  const body = resolveNotesBodyWithPatchFallback(markdown, version);
  if (body) {
    return `## Aero Media Service ${version}\n\n${body}`;
  }
  if (isPrereleaseVersion(version)) {
    return `## Aero Media Service ${version}\n\n### Hinweis\n\n- Vorabversion zum Testen`;
  }
  return `Aero Media Service ${version}`;
}

/**
 * Prefer exact version section; if missing/empty and version is stable X.Y.Z,
 * reuse the nearest older section with the same major.minor (patch walk-back),
 * else the chronologically previous versioned section.
 * Prerelease: exact only (no walk-back).
 */
export function resolveNotesBodyWithPatchFallback(markdown, version) {
  const exact = extractSectionBody(markdown, version);
  if (hasMeaningfulNotes(exact ?? "")) return exact;

  if (isPrereleaseVersion(version)) {
    return null;
  }

  const sections = parseSections(markdown).filter((s) => s.version !== "Unreleased");
  const semver = parseSemVerLoose(version);
  if (semver && !semver.prerelease) {
    const sameMinor = sections
      .map((s) => ({ s, v: parseSemVerLoose(s.version) }))
      .filter(
        (x) =>
          x.v &&
          !x.v.prerelease &&
          x.v.major === semver.major &&
          x.v.minor === semver.minor &&
          x.v.patch < semver.patch,
      )
      .sort((a, b) => b.v.patch - a.v.patch);
    for (const { s } of sameMinor) {
      const body = markdown.slice(s.bodyStart, s.end).trim();
      if (hasMeaningfulNotes(body)) return body;
    }
  }

  for (const s of sections) {
    if (isPrereleaseVersion(s.version)) continue;
    const body = markdown.slice(s.bodyStart, s.end).trim();
    if (hasMeaningfulNotes(body)) return body;
  }
  return null;
}

function todayIso() {
  const d = new Date();
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

export function hasMeaningfulNotes(body) {
  if (!body) return false;
  return /(?:^|\n)\s*[-*+]\s+\S/.test(body) || /(?:^|\n)\s*[^#\s-][^\n]{8,}/.test(body);
}

const BETA_STUB = `### Hinweis

- Vorabversion zum Testen`;

/**
 * Resolve notes body for a release bump.
 * - Prefer [Unreleased]
 * - channel "beta": Unreleased optional → stub; never requires previous patch copy
 * - patch stable only: if Unreleased empty, copy previousVersion section
 * - minor/major stable: Unreleased required
 *
 * @returns {{ body: string, source: "unreleased" | "previous" | "stub", fromVersion?: string }}
 */
export function resolveNotesForRelease(markdown, kind, previousVersion, channel = "stable") {
  const unreleased = extractSectionBody(markdown, "Unreleased");
  if (hasMeaningfulNotes(unreleased ?? "")) {
    return { body: unreleased, source: "unreleased" };
  }

  if (channel === "beta") {
    return { body: BETA_STUB, source: "stub" };
  }

  if (kind === "patch") {
    const prevCore = previousVersion.replace(/-.*$/, "");
    const prev = extractSectionBody(markdown, previousVersion)
      ?? extractSectionBody(markdown, prevCore);
    if (hasMeaningfulNotes(prev ?? "")) {
      return { body: prev, source: "previous", fromVersion: previousVersion };
    }
    throw new Error(
      `CHANGELOG.md: [Unreleased] leer und keine Notes für ${previousVersion}.\n` +
        "Bitte Notes unter ## [Unreleased] oder unter der Vorgängerversion ergänzen.",
    );
  }

  throw new Error(
    "CHANGELOG.md: [Unreleased] ist leer.\n" +
      "Bei minor/major bitte Nutzer-Notes unter ## [Unreleased] eintragen, committen, dann erneut release starten.",
  );
}

/**
 * Insert ## [version] with body after clearing [Unreleased] (stable promote).
 */
export function insertVersionNotes(markdown, version, body, date = todayIso()) {
  const sections = parseSections(markdown);
  const unreleased = sections.find((s) => s.version === "Unreleased");
  if (!unreleased) {
    throw new Error("CHANGELOG.md: Abschnitt ## [Unreleased] fehlt");
  }
  if (!hasMeaningfulNotes(body)) {
    throw new Error("CHANGELOG.md: Notes-Body ist leer");
  }
  if (sections.some((s) => s.version === version)) {
    throw new Error(`CHANGELOG.md: Abschnitt ## [${version}] existiert bereits`);
  }

  const before = markdown.slice(0, unreleased.start);
  const after = markdown.slice(unreleased.end);
  const unreleasedBlock = `## [Unreleased]\n\n`;
  const versionBlock = `## [${version}] - ${date}\n\n${body.trim()}\n\n`;
  return `${before}${unreleasedBlock}${versionBlock}${after.replace(/^\n+/, "")}`;
}

/**
 * Insert ## [beta-version] snapshot after [Unreleased], keeping Unreleased body intact.
 */
export function insertBetaSnapshot(markdown, version, body, date = todayIso()) {
  const sections = parseSections(markdown);
  const unreleased = sections.find((s) => s.version === "Unreleased");
  if (!unreleased) {
    throw new Error("CHANGELOG.md: Abschnitt ## [Unreleased] fehlt");
  }
  if (!hasMeaningfulNotes(body)) {
    throw new Error("CHANGELOG.md: Notes-Body ist leer");
  }
  if (sections.some((s) => s.version === version)) {
    throw new Error(`CHANGELOG.md: Abschnitt ## [${version}] existiert bereits`);
  }

  const head = markdown.slice(0, unreleased.end).replace(/\n+$/, "\n\n");
  const after = markdown.slice(unreleased.end).replace(/^\n+/, "");
  const versionBlock = `## [${version}] - ${date}\n\n${body.trim()}\n\n`;
  return `${head}${versionBlock}${after}`;
}

/**
 * Move [Unreleased] body into ## [version] - date and leave Unreleased empty.
 * Throws if Unreleased is missing or empty.
 */
export function promoteUnreleased(markdown, version, date = todayIso()) {
  const unreleased = extractSectionBody(markdown, "Unreleased");
  if (!hasMeaningfulNotes(unreleased ?? "")) {
    throw new Error(
      "CHANGELOG.md: [Unreleased] ist leer. Bitte Release-Notes eintragen, dann erneut release starten.",
    );
  }
  return insertVersionNotes(markdown, version, unreleased, date);
}

export function readChangelog(path = CHANGELOG_PATH) {
  return readFileSync(path, "utf8");
}

export function writeChangelog(markdown, path = CHANGELOG_PATH) {
  writeFileSync(path, markdown.endsWith("\n") ? markdown : `${markdown}\n`, "utf8");
}

function runCli() {
  const [cmd, version] = process.argv.slice(2);
  if (cmd === "extract" && version) {
    const notes = releaseNotesForVersion(readChangelog(), version);
    process.stdout.write(`${notes}\n`);
    return;
  }
  if (cmd === "promote" && version) {
    const next = promoteUnreleased(readChangelog(), version);
    writeChangelog(next);
    console.error(`Promoted [Unreleased] → [${version}]`);
    return;
  }
  console.error("Usage: node scripts/changelog.mjs extract <version>");
  console.error("       node scripts/changelog.mjs promote <version>");
  process.exit(1);
}

const entry = process.argv[1] ? pathToFileURL(process.argv[1]).href : "";
if (entry && import.meta.url === entry) {
  runCli();
}
