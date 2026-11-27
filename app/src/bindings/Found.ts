// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type Found = { 
/**
 * Unique within one scan, e.g. `steam:123:4000000000`.
 */
id: string, name: string, 
/**
 * `steam`, `epic`, `gog`, `xbox`, `recomplauncher`, `emulator` (saves in
 * an emulator's data folder), `ludusavi` or `folder`.
 */
source: "steam" | "epic" | "gog" | "xbox" | "recomplauncher" | "emulator" | "ludusavi" | "folder", exe: string | null, install: string | null, args: string, steamAccount: string | null, steamAppid: number | null, adapter: string | null, adapterName: string | null, 
/**
 * Save patterns the scan worked out: one emulator game's saves, or a
 * save folder picked in RecompLauncher (empty: the rule's or unknown).
 */
saves: Array<string>, ludusaviTitle: string | null, 
/**
 * Save files found on this PC (0 when unknown).
 */
saveFiles: number, saveBytes: number, saveSample: Array<string>, 
/**
 * Steam Cloud already keeps these saves in sync.
 */
steamCloud: boolean, 
/**
 * A store game installed through Steam.
 */
storeGame: boolean, 
/**
 * Its app id in Steam, when it is known which installed game it is:
 * such a game is added as one Steam installed ([`crate::steamcloud`]).
 */
storeAppid: number | null, 
/**
 * Key of the library entry for this game, when already added.
 */
addedKey: string | null, 
/**
 * Saves only in the registry (not supported yet).
 */
registryOnly: boolean, 
/**
 * Where the install was found for a game known by its saves ("epic",
 * "gog", "xbox", "registry", "folder").
 */
installStore: string | null, 
/**
 * Normalized name for matching results from different sources.
 */
matchKey: string, };
