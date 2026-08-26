import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  bumpCore,
  compareSemVer,
  isPrereleaseVersion,
  nextBetaVersion,
  toStableVersion,
} from "./semver.mjs";
import {
  insertBetaSnapshot,
  insertVersionNotes,
  resolveNotesForRelease,
} from "./changelog.mjs";

describe("semver", () => {
  it("orders prerelease below release", () => {
    assert.ok(compareSemVer("0.1.14-beta.1", "0.1.14") < 0);
    assert.ok(compareSemVer("0.1.14", "0.1.14-beta.1") > 0);
    assert.ok(compareSemVer("0.1.14-beta.1", "0.1.14-beta.2") < 0);
    assert.equal(compareSemVer("v0.1.14-beta.1", "0.1.14-beta.1"), 0);
  });

  it("bumps beta and promotes to stable", () => {
    assert.equal(nextBetaVersion("0.1.13", "patch"), "0.1.14-beta.1");
    assert.equal(nextBetaVersion("0.1.14-beta.1"), "0.1.14-beta.2");
    assert.equal(toStableVersion("0.1.14-beta.2"), "0.1.14");
    assert.equal(bumpCore("0.1.13", "patch"), "0.1.14");
    assert.ok(isPrereleaseVersion("0.1.14-beta.1"));
    assert.ok(!isPrereleaseVersion("0.1.14"));
  });
});

describe("changelog beta/stable", () => {
  const base = `## [Unreleased]

### Neu
- Feature A

## [0.1.13] - 2026-08-01

### Behoben
- Fix B
`;

  it("snapshots beta without clearing Unreleased", () => {
    const next = insertBetaSnapshot(base, "0.1.14-beta.1", "### Neu\n- Feature A");
    assert.match(next, /## \[Unreleased\]\s*\n\s*### Neu\s*\n- Feature A/);
    assert.match(next, /## \[0\.1\.14-beta\.1\]/);
  });

  it("promotes stable and clears Unreleased", () => {
    const next = insertVersionNotes(base, "0.1.14", "### Neu\n- Feature A");
    assert.match(next, /## \[Unreleased\]\s*\n\s*## \[0\.1\.14\]/);
    assert.doesNotMatch(next, /## \[Unreleased\][\s\S]*Feature A[\s\S]*## \[0\.1\.14\]/);
  });

  it("uses stub when beta and Unreleased empty", () => {
    const empty = `## [Unreleased]\n\n## [0.1.13] - 2026-08-01\n\n- x\n`;
    const { source, body } = resolveNotesForRelease(empty, "patch", "0.1.13", "beta");
    assert.equal(source, "stub");
    assert.match(body, /Vorabversion/);
  });
});
