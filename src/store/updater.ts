/**
 * Auto-updater store — REMOVED.
 *
 * The tauri-plugin-updater was intentionally removed from Cargo.toml
 * because Kilroy is a fully-local app not designed to access the internet.
 * The frontend store, palette commands, and tauri.conf.json updater stubs
 * have been removed to prevent runtime import failures in production builds.
 *
 * Manual update path: rebuild from source via `npm run build:release`.
 */
export {};