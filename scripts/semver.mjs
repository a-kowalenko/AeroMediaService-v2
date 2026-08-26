/**
 * Shared SemVer helpers for release.mjs / changelog.mjs / CI.
 * Keep in sync with src/lib/versionCompare.ts semantics.
 */

/**
 * @param {string} version
 * @returns {boolean}
 */
export function isPrereleaseVersion(version) {
  const v = String(version).trim().replace(/^v/i, "");
  return /-/.test(v);
}

/**
 * @param {string} input
 * @returns {{ major: number, minor: number, patch: number, prerelease: (string|number)[] | null }}
 */
export function parseSemVer(input) {
  const v = String(input).trim().replace(/^v/i, "");
  const m = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+.*)?$/.exec(v);
  if (!m) {
    throw new Error(`Ungültige Version: ${input}`);
  }
  const prerelease = m[4]
    ? m[4].split(".").map((id) => (/^\d+$/.test(id) ? Number.parseInt(id, 10) : id))
    : null;
  return {
    major: Number(m[1]),
    minor: Number(m[2]),
    patch: Number(m[3]),
    prerelease,
  };
}

/**
 * Loose parse that returns null instead of throwing (changelog walk-back).
 * @param {string} v
 */
export function parseSemVerLoose(v) {
  try {
    return parseSemVer(v);
  } catch {
    return null;
  }
}

function comparePrereleaseId(a, b) {
  const aNum = typeof a === "number";
  const bNum = typeof b === "number";
  if (aNum && bNum) return a - b;
  if (aNum) return -1;
  if (bNum) return 1;
  return String(a).localeCompare(String(b));
}

function comparePrerelease(a, b) {
  if (a == null && b == null) return 0;
  if (a == null) return 1;
  if (b == null) return -1;
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i++) {
    if (i >= a.length) return -1;
    if (i >= b.length) return 1;
    const c = comparePrereleaseId(a[i], b[i]);
    if (c !== 0) return c;
  }
  return 0;
}

/**
 * @param {string} a
 * @param {string} b
 * @returns {number} <0, 0, >0
 */
export function compareSemVer(a, b) {
  const left = parseSemVer(a);
  const right = parseSemVer(b);
  if (left.major !== right.major) return left.major - right.major;
  if (left.minor !== right.minor) return left.minor - right.minor;
  if (left.patch !== right.patch) return left.patch - right.patch;
  return comparePrerelease(left.prerelease, right.prerelease);
}

/**
 * Format core + optional prerelease.
 * @param {{ major: number, minor: number, patch: number, prerelease?: (string|number)[] | null }} s
 */
export function formatSemVer(s) {
  const core = `${s.major}.${s.minor}.${s.patch}`;
  if (!s.prerelease || s.prerelease.length === 0) return core;
  return `${core}-${s.prerelease.join(".")}`;
}

/**
 * Bump core version (strips any prerelease).
 * @param {string} version
 * @param {"major"|"minor"|"patch"} kind
 */
export function bumpCore(version, kind) {
  const s = parseSemVer(version);
  if (kind === "major") return `${s.major + 1}.0.0`;
  if (kind === "minor") return `${s.major}.${s.minor + 1}.0`;
  if (kind === "patch") return `${s.major}.${s.minor}.${s.patch + 1}`;
  throw new Error(`Unbekannter Bump-Typ: ${kind}`);
}

/**
 * Next beta version from current.
 * - Stable 0.1.13 + kind patch → 0.1.14-beta.1
 * - Already 0.1.14-beta.1 → 0.1.14-beta.2 (kind ignored)
 * @param {string} version
 * @param {"major"|"minor"|"patch"} [kind]
 */
export function nextBetaVersion(version, kind = "patch") {
  const s = parseSemVer(version);
  if (s.prerelease) {
    const ids = [...s.prerelease];
    const last = ids[ids.length - 1];
    if (typeof last === "number") {
      ids[ids.length - 1] = last + 1;
    } else if (ids.length === 1 && ids[0] === "beta") {
      ids.push(1);
    } else {
      ids.push(1);
    }
    return formatSemVer({ ...s, prerelease: ids });
  }
  const core = bumpCore(version, kind);
  return `${core}-beta.1`;
}

/**
 * Strip prerelease → stable core (0.1.14-beta.2 → 0.1.14).
 * @param {string} version
 */
export function toStableVersion(version) {
  const s = parseSemVer(version);
  if (!s.prerelease) {
    throw new Error(`Version ${version} ist bereits stabil (kein -beta/-rc).`);
  }
  return formatSemVer({ ...s, prerelease: null });
}
