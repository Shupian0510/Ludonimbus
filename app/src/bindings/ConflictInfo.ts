// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type ConflictInfo = { 
/**
 * The newest of the versions to choose from.
 */
cloudId: string, cloudDevice: string, cloudCreatedAt: string, localHash: string, detectedAt: string, 
/**
 * Every version to choose from, oldest first: one normally, several
 * when the line itself has forked.
 */
candidates?: Array<string>, 
/**
 * The saves on this device are one more choice.
 */
localIsCandidate: boolean, };
