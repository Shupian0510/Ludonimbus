// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type SaveCandidate = { path: string, 
/**
 * The save pattern to use for it (`{localappdata}/Game/Saved/SaveGames/**`).
 */
pattern: string, files: number, bytes: number, modified: string | null, 
/**
 * A few file names inside, for the user to recognise.
 */
sample: Array<string>, 
/**
 * Why it is suggested (shown to the user).
 */
reason: string, 
/**
 * The game's engine or Steam says this is where it keeps saves (it may
 * not have saved yet: no files).
 */
likely: boolean, };
