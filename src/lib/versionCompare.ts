/**
 * SemVer compare (0.1.10 > 0.1.9; 0.1.14-beta.1 < 0.1.14).
 * Leading `v` is ignored. Returns <0, 0, >0.
 * Keep in sync with scripts/semver.mjs and src-tauri updater SemVer.
 */

export type SemVerParts = {
  major: number;
  minor: number;
  patch: number;
  /** null = release (no prerelease). */
  prerelease: (string | number)[] | null;
};

/** True if the version string has a SemVer prerelease suffix (`-beta.1`, `-rc.2`, …). */
export function isVersionPrerelease(version: string): boolean {
  const v = version.trim().replace(/^v/i, "");
  return /-/.test(v);
}

export function parseSemVer(input: string): SemVerParts {
  const v = input.trim().replace(/^v/i, "");
  const m = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+.*)?$/.exec(v);
  if (m) {
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
  // Fallback: numeric segments only (legacy / odd tags).
  const nums = v
    .split(/[^\d]+/)
    .filter(Boolean)
    .map((s) => Number.parseInt(s, 10))
    .filter((n) => Number.isFinite(n));
  return {
    major: nums[0] ?? 0,
    minor: nums[1] ?? 0,
    patch: nums[2] ?? 0,
    prerelease: null,
  };
}

function comparePrereleaseId(a: string | number, b: string | number): number {
  const aNum = typeof a === "number";
  const bNum = typeof b === "number";
  if (aNum && bNum) return (a as number) - (b as number);
  if (aNum) return -1; // numeric < non-numeric
  if (bNum) return 1;
  return String(a).localeCompare(String(b));
}

function comparePrerelease(
  a: (string | number)[] | null,
  b: (string | number)[] | null,
): number {
  // Release (null) > any prerelease
  if (a == null && b == null) return 0;
  if (a == null) return 1;
  if (b == null) return -1;
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i++) {
    if (i >= a.length) return -1;
    if (i >= b.length) return 1;
    const c = comparePrereleaseId(a[i]!, b[i]!);
    if (c !== 0) return c;
  }
  return 0;
}

export function compareVersionParts(a: string, b: string): number {
  const left = parseSemVer(a);
  const right = parseSemVer(b);
  if (left.major !== right.major) return left.major - right.major;
  if (left.minor !== right.minor) return left.minor - right.minor;
  if (left.patch !== right.patch) return left.patch - right.patch;
  return comparePrerelease(left.prerelease, right.prerelease);
}
