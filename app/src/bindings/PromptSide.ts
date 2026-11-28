// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * One of the two saves of a [`Compare`].
 */
export type PromptSide = { 
/**
 * What it is called: "GOG 版", "云端", "本机"…
 */
name: string, 
/**
 * What it was made on ([`crate::sync::platform`]): which program's
 * icon the window shows. Empty when not known.
 */
platform: string, 
/**
 * The device that made it.
 */
device: string, at: string | null, 
/**
 * How long the game had been played when it was made, in seconds.
 */
playtime: number | null, size: number | null, files: number | null, 
/**
 * The newer of the two: the window stresses it.
 */
newer: boolean, };
