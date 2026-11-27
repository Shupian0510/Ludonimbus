// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * One folder of a save line, and the folder that holds the same files in
 * a version of the game.
 */
export type SaveBind = { 
/**
 * The folder in the line's files.
 */
canon: string, 
/**
 * The folder in the version's own files.
 */
local: string, 
/**
 * How the version keeps them. Plain files unless it says otherwise,
 * and then not written, so bindings of plain files read as before.
 * In JSON a string (`files`, `wgs`, or a newer Ludonimbus's codec).
 */
codec?: string, 
/**
 * Which files in the folder belong to the binding: a wildcard relative
 * to it, matched as a cross-platform rule's `files` are (any letter
 * case, `*` doesn't cross `/`). None = every file in it, and then not
 * written, so bindings without one read and write as before.
 */
files?: string | null, };
