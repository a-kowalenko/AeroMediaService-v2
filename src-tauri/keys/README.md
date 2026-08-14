# Updater signing keys

- `updater.key` — **private**, gitignored. Copy contents into GitHub secret `TAURI_SIGNING_PRIVATE_KEY`.
- `updater.key.pub` — public; also embedded in `tauri.conf.json` → `plugins.updater.pubkey`.

See `docs/RELEASE.md`.
