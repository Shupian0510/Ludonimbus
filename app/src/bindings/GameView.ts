// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { ArtView } from "./ArtView";
import type { ConflictInfo } from "./ConflictInfo";
import type { GameSource } from "./GameSource";
import type { Health } from "./Health";
import type { PlaytimeSummary } from "./PlaytimeSummary";
import type { ReplacedView } from "./ReplacedView";
import type { StatusReport } from "./StatusReport";
import type { SteamView } from "./SteamView";
import type { StoreView } from "./StoreView";

export type GameView = { key: string, name: string, adapter: string | null, adapterName: string | null, platform: string | null, exe: string | null, exeExists: boolean, 
/**
 * The game itself is on this PC; otherwise the library shows it grey,
 * as saves only (see `games::playable`).
 */
playable: boolean, install: string | null, args: string, saves: Array<string>, exclude: Array<string>, waitFor: Array<string>, customSaves: boolean, defaultSaves: Array<string>, defaultWaitFor: Array<string>, 
/**
 * Effective: upload when the game process exits, not when the launcher closes.
 */
uploadOnGameExit: boolean, hasGameProcess: boolean, ludusaviTitle: string | null, autoSync: boolean, steam: SteamView | null, health: Health, art: ArtView, running: boolean, lastSyncAt: string | null, lastSyncedDevice: string | null, conflict: ConflictInfo | null, lastError: string | null, pending: number, 
/**
 * The last check of the game, as `check_game` returned it (kept as JSON
 * in the game's state, `saves::cache_check`).
 */
lastCheck: StatusReport | null, 
/**
 * Playtime over all devices (from local data; refreshed by every sync).
 */
playtime: PlaytimeSummary, 
/**
 * Emulator / recomp / PC game…, for the library's badges and groups.
 */
source: GameSource, 
/**
 * Which game this is a version of: the key of the first of its
 * versions in the save library (see `savelib::families`).
 */
family: string, 
/**
 * The user's "same game" link (`solo:…` = kept apart).
 */
link: string | null, 
/**
 * It shares the save line of another version through matched folders
 * (`join`), which the user can take back.
 */
joined: boolean, 
/**
 * It reads and writes that line through the Xbox app's save containers
 * (`bind::Codec::Wgs`): writing into them is experimental, and the app
 * says so before it does.
 */
xboxContainers: boolean, 
/**
 * The last time saves of the cloud replaced the ones here, while that
 * can still be undone (`sync::undo`).
 */
replaced: ReplacedView | null, 
/**
 * For a game Steam installed: who keeps its saves.
 */
store: StoreView | null, 
/**
 * The collection's shelves its card is on (`finished`, `archived`).
 */
shelves?: Array<string>, 
/**
 * Newer saves of its own line come here by themselves (through Steam,
 * or Steam Cloud): a newer version in the cloud needs nothing from the
 * user ([`auto_pull`]).
 */
autoPull: boolean, };
