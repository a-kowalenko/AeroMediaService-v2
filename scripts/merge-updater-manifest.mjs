/**
 * Build / merge Tauri updater latest.json from GitHub release assets.
 *
 * CI runs this after all platform builds finish so parallel matrix jobs cannot
 * overwrite each other's platform keys. Also used to repair broken manifests.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

export const RELEASES_OWNER = "a-kowalenko";
export const RELEASES_REPO = "aero-media-service-releases";
const USER_AGENT = "AeroMediaService-Updater-Manifest";

/**
 * @param {string} name
 * @returns {string | null}
 */
export function assetApiUrl(name, assetId) {
  return `https://api.github.com/repos/${RELEASES_OWNER}/${RELEASES_REPO}/releases/assets/${assetId}`;
}

/**
 * @param {{ id: number, name: string }[]} assets
 * @param {Map<string, string>} signatures
 * @returns {Record<string, { signature: string, url: string }>}
 */
export function buildPlatformEntries(assets, signatures) {
  /** @type {Record<string, { signature: string, url: string }>} */
  const platforms = {};

  /**
   * @param {string} key
   * @param {string} assetName
   */
  function add(key, assetName) {
    const asset = assets.find((a) => a.name === assetName);
    const signature = signatures.get(`${assetName}.sig`);
    if (!asset || !signature) return;
    platforms[key] = {
      signature,
      url: assetApiUrl(asset.name, asset.id),
    };
  }

  for (const asset of assets) {
    const name = asset.name;
    const lower = name.toLowerCase();
    if (lower.endsWith(".sig") || lower === "latest.json") continue;

    if (lower.endsWith(".app.tar.gz")) {
      if (name.includes("_aarch64")) {
        add("darwin-aarch64", name);
        add("darwin-aarch64-app", name);
      } else if (name.includes("_x64") || name.includes("x86_64")) {
        add("darwin-x86_64", name);
        add("darwin-x86_64-app", name);
      }
      continue;
    }

    if (lower.endsWith(".appimage")) {
      add("linux-x86_64", name);
      add("linux-x86_64-appimage", name);
      continue;
    }

    if (lower.endsWith("-setup.exe")) {
      add("windows-x86_64", name);
      add("windows-x86_64-nsis", name);
      continue;
    }

    if (lower.endsWith(".msi")) {
      add("windows-x86_64-msi", name);
    }
  }

  return platforms;
}

/**
 * @param {Record<string, unknown>} platforms
 * @returns {string[]}
 */
export function missingRequiredPlatformKeys(platforms) {
  const required = [
    "darwin-aarch64",
    "darwin-aarch64-app",
    "darwin-x86_64",
    "darwin-x86_64-app",
    "linux-x86_64",
    "linux-x86_64-appimage",
    "windows-x86_64",
    "windows-x86_64-nsis",
  ];
  return required.filter((key) => !(key in platforms));
}

/**
 * @param {string} tag
 * @param {{ token?: string, releaseNotes?: string }} [opts]
 */
export async function buildUpdaterManifest(tag, opts = {}) {
  const normalizedTag = tag.startsWith("v") ? tag : `v${tag}`;
  const version = normalizedTag.replace(/^v/, "");
  const headers = {
    Accept: "application/vnd.github+json",
    "User-Agent": USER_AGENT,
    "X-GitHub-Api-Version": "2022-11-28",
  };
  if (opts.token) {
    headers.Authorization = `Bearer ${opts.token}`;
  }

  const releaseRes = await fetch(
    `https://api.github.com/repos/${RELEASES_OWNER}/${RELEASES_REPO}/releases/tags/${normalizedTag}`,
    { headers },
  );
  if (!releaseRes.ok) {
    throw new Error(
      `Release ${normalizedTag} not found (${releaseRes.status} ${releaseRes.statusText})`,
    );
  }
  /** @type {{ assets: { id: number, name: string, browser_download_url: string }[], body?: string, published_at?: string }} */
  const release = await releaseRes.json();
  const assets = release.assets ?? [];

  /** @type {Map<string, string>} */
  const signatures = new Map();
  for (const asset of assets) {
    if (!asset.name.endsWith(".sig")) continue;
    const sigRes = await fetch(asset.browser_download_url, {
      headers: { "User-Agent": USER_AGENT },
    });
    if (!sigRes.ok) {
      throw new Error(`Failed to fetch signature ${asset.name}: ${sigRes.status}`);
    }
    const signature = (await sigRes.text()).trim();
    if (!signature) {
      throw new Error(`Empty signature file: ${asset.name}`);
    }
    signatures.set(asset.name, signature);
  }

  const platforms = buildPlatformEntries(assets, signatures);
  const missing = missingRequiredPlatformKeys(platforms);
  if (missing.length > 0) {
    throw new Error(
      `Manifest incomplete for ${normalizedTag}; missing platform keys: ${missing.join(", ")}`,
    );
  }

  const notes =
    (opts.releaseNotes ?? release.body ?? "").trim() ||
    `Aero Media Service ${version}`;

  return {
    version,
    notes,
    pub_date: release.published_at ?? new Date().toISOString(),
    platforms,
  };
}

/**
 * @param {string} tag
 * @param {string} outputPath
 * @param {{ token?: string, releaseNotes?: string }} [opts]
 */
export async function writeUpdaterManifest(tag, outputPath, opts = {}) {
  const manifest = await buildUpdaterManifest(tag, opts);
  writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
  return manifest;
}

/**
 * @param {string} tag
 * @param {string} filePath
 * @param {string} token
 */
export function uploadUpdaterManifest(tag, filePath, token) {
  const normalizedTag = tag.startsWith("v") ? tag : `v${tag}`;
  const repo = `${RELEASES_OWNER}/${RELEASES_REPO}`;
  const result = spawnSync(
    "gh",
    ["release", "upload", normalizedTag, filePath, "--repo", repo, "--clobber"],
    {
      env: { ...process.env, GH_TOKEN: token, GITHUB_TOKEN: token },
      encoding: "utf8",
    },
  );
  if (result.status !== 0) {
    throw new Error(
      `gh release upload failed: ${result.stderr || result.stdout || result.status}`,
    );
  }
}

function parseArgs(argv) {
  const args = { tag: null, out: "latest.json", upload: false, notesFile: null };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--tag") args.tag = argv[++i] ?? null;
    else if (arg === "--out") args.out = argv[++i] ?? args.out;
    else if (arg === "--upload") args.upload = true;
    else if (arg === "--notes-file") args.notesFile = argv[++i] ?? null;
  }
  return args;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.tag) {
    console.error(
      "Usage: node scripts/merge-updater-manifest.mjs --tag v0.1.10 [--out latest.json] [--upload] [--notes-file notes.md]",
    );
    process.exit(1);
  }

  const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN || "";
  const releaseNotes = args.notesFile
    ? readFileSync(args.notesFile, "utf8").trim()
    : undefined;

  const manifest = await writeUpdaterManifest(args.tag, args.out, {
    token: token || undefined,
    releaseNotes,
  });

  console.error(
    `Wrote ${args.out} for ${args.tag} with ${Object.keys(manifest.platforms).length} platform keys`,
  );

  if (args.upload) {
    if (!token) {
      throw new Error("Missing GH_TOKEN / GITHUB_TOKEN for --upload");
    }
    uploadUpdaterManifest(args.tag, args.out, token);
    console.error(`Uploaded ${args.out} to ${RELEASES_OWNER}/${RELEASES_REPO}`);
  }
}

const entry = process.argv[1] ?? "";
if (entry.endsWith("merge-updater-manifest.mjs")) {
  main().catch((err) => {
    console.error(err instanceof Error ? err.message : String(err));
    process.exit(1);
  });
}
