// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * A version made on this device that couldn't be uploaded complete: a pack
 * it took files from was gone from the cloud, and their content was nowhere
 * any more. What it brought itself is kept in local-backups; the game's
 * saves tab shows it once, until the user dismisses it.
 */
export type UnsentSave = { id: string, createdAt: string, totalSize: number, fileCount: number, platform?: string, 
/**
 * When it was taken off the queue.
 */
at: string, };
