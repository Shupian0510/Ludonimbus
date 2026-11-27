// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { CloudGameBackup } from "./CloudGameBackup";
import type { PlaytimeSummary } from "./PlaytimeSummary";

export type CloudGameInfo = { playtime: PlaytimeSummary, 
/**
 * The newest backup of the game itself, when there is one.
 */
gameBackup: CloudGameBackup | null, };
