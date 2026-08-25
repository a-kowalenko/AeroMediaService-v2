import test from "node:test";
import assert from "node:assert/strict";
import {
  buildPlatformEntries,
  missingRequiredPlatformKeys,
} from "./merge-updater-manifest.mjs";

test("buildPlatformEntries maps release assets to Tauri platform keys", () => {
  const assets = [
    { id: 1, name: "Aero.Media.Service_0.1.10_aarch64.app.tar.gz" },
    { id: 2, name: "Aero.Media.Service_0.1.10_aarch64.app.tar.gz.sig" },
    { id: 3, name: "Aero.Media.Service_0.1.10_x64.app.tar.gz" },
    { id: 4, name: "Aero.Media.Service_0.1.10_x64.app.tar.gz.sig" },
    { id: 5, name: "Aero.Media.Service_0.1.10_amd64.AppImage" },
    { id: 6, name: "Aero.Media.Service_0.1.10_amd64.AppImage.sig" },
    { id: 7, name: "Aero.Media.Service_0.1.10_x64-setup.exe" },
    { id: 8, name: "Aero.Media.Service_0.1.10_x64-setup.exe.sig" },
    { id: 9, name: "Aero.Media.Service_0.1.10_x64_en-US.msi" },
    { id: 10, name: "Aero.Media.Service_0.1.10_x64_en-US.msi.sig" },
    { id: 11, name: "latest.json" },
  ];
  const signatures = new Map(
    assets
      .filter((a) => a.name.endsWith(".sig"))
      .map((a) => [a.name, `sig-${a.name}`]),
  );

  const platforms = buildPlatformEntries(assets, signatures);

  assert.equal(platforms["darwin-aarch64"]?.signature, "sig-Aero.Media.Service_0.1.10_aarch64.app.tar.gz.sig");
  assert.equal(platforms["darwin-x86_64-app"]?.url, platforms["darwin-x86_64"]?.url);
  assert.equal(
    platforms["windows-x86_64-nsis"]?.url,
    "https://api.github.com/repos/a-kowalenko/aero-media-service-releases/releases/assets/7",
  );
  assert.equal(missingRequiredPlatformKeys(platforms).length, 0);
});

test("missingRequiredPlatformKeys detects broken macOS manifests", () => {
  const platforms = {
    "linux-x86_64": {},
    "windows-x86_64": {},
  };
  const missing = missingRequiredPlatformKeys(platforms);
  assert.ok(missing.some((key) => key.startsWith("darwin-")));
});
