// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * Game definition stored in the cloud so other devices can recognise it.
 */
export type CloudGame = { key: string, name: string, adapter: string | null, saves: Array<string>, exclude: Array<string>, waitFor: Array<string>, roots: { [key in string]: string }, ludusaviTitle: string | null, 
/**
 * The library group the user chose, so other devices show the game
 * in the same group.
 */
source?: string | null, 
/**
 * What identifies the game on any platform (`nx:…`, `ludusavi:…`,
 * `xbox:…`, `name:…`), so other devices can tell versions of the same
 * game apart from other games (see [`crate::savelib`]).
 */
ids?: Array<string>, 
/**
 * The user's "same game as" link ([`GameEntry::game`]).
 */
game?: string | null, };
