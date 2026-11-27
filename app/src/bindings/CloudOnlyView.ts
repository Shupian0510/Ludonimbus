// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { ArtView } from "./ArtView";
import type { GameSource } from "./GameSource";
import type { PlaytimeSummary } from "./PlaytimeSummary";

/**
 * A game the cloud has but this PC doesn't: shown grey in the library.
 */
export type CloudOnlyView = { key: string, name: string, adapter: string | null, adapterName: string | null, platform: string | null, ludusaviTitle: string | null, saves: Array<string>, source: GameSource, 
/**
 * Artwork downloaded from the cloud (see `artsync`).
 */
art: ArtView, 
/**
 * From this PC's copy of the other devices' playtime.
 */
playtime: PlaytimeSummary, family: string, link: string | null, shelves?: Array<string>, };
