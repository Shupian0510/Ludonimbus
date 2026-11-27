// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { MillenniumInstall } from "./MillenniumInstall";

export type SteamPluginStatus = { steamRoot: string | null, steamRunning: boolean, 
/**
 * This build ships the plugin.
 */
bundled: boolean, millennium: MillenniumInstall | null, pluginInstalled: boolean, pluginEnabled: boolean, pluginCurrent: boolean, };
