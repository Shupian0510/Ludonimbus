// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * Where a restore puts things.
 */
export type Targets = { 
/**
 * The program's folder (or the whole folder). Also where the game's
 * settings that live next to the program go.
 */
program: string, 
/**
 * Write the program's files; `false` keeps the program already there.
 */
restoreProgram: boolean, 
/**
 * The folder for the game's own files; `None` = don't restore them.
 */
content: string | null, };
