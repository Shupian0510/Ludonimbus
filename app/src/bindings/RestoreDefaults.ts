// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * Where a restore goes by default and what is already on this PC.
 */
export type RestoreDefaults = { 
/**
 * The version has a program (or whole folder) part.
 */
hasProgram: boolean, 
/**
 * The version has the game's own files.
 */
hasContent: boolean, programName: string | null, programVersion: string | null, 
/**
 * A folder on this PC with this game's program in it.
 */
installedProgram: string | null, 
/**
 * That program is the backed-up one (same exe).
 */
installedSame: boolean, 
/**
 * Where the program goes by default (the backed-up folder when it is free).
 */
programDir: string | null, 
/**
 * Where the game's files go by default: where they were, else where this
 * emulator's games were last restored to.
 */
contentDir: string | null, };
