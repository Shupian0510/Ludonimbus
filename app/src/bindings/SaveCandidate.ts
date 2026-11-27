// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type SaveCandidate = { path: string, files: number, bytes: number, modified: string | null, 
/**
 * A few file names inside, for the user to recognise.
 */
sample: Array<string>, 
/**
 * Why it is suggested (shown to the user).
 */
reason: string, };
