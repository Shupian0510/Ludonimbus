// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * A folder a sync client (OneDrive, Dropbox, Google Drive for desktop…)
 * keeps in the cloud, offered as a place for the "local folder" cloud.
 */
export type SyncFolder = { 
/**
 * The sync client, e.g. "OneDrive".
 */
label: string, 
/**
 * `Ludonimbus` inside the synced folder: where saves would go.
 */
path: string, 
/**
 * That folder is already there (another PC may have put saves in it).
 */
existing: boolean, };
