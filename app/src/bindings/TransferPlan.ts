// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { TransferItem } from "./TransferItem";

export type TransferPlan = { 
/**
 * Same save layout: applied like a restore.
 */
same: boolean, fromXbox: boolean, toXbox: boolean, 
/**
 * The game's rule in `xbox-saves.yaml` (`None`: the generic rule).
 */
xboxRule: string | null, items: Array<TransferItem>, 
/**
 * Source files with no place in the target (Xbox: no such save slot).
 */
unmatched: Array<string>, 
/**
 * The target folder the files go into.
 */
base: string | null, 
/**
 * How many files land on a file the target already has.
 */
matched: number, 
/**
 * The target has no saves yet: play it once and save, or pick its save
 * folder.
 */
needsFolder: boolean, 
/**
 * The two versions have different save formats ([`savelib::save_scope`]):
 * the files are rarely usable as they are. Only versions the user linked
 * by hand get here.
 */
crossPlatform: boolean, 
/**
 * The rule both versions follow (`rules/games/`): the saves
 * go into the rule's folder file for file, whatever the platforms.
 */
formatRule: string | null, 
/**
 * Source files that aren't saves under that rule (settings…): left out.
 */
skipped: Array<string>, 
/**
 * The game version that wrote the source's saves, and the target's
 * current saves (where the rule says how to read it).
 */
sourceVersion: string | null, targetVersion: string | null, 
/**
 * The source's saves are from a newer game version than the target's,
 * and the game can't load saves of a newer version: the target may need
 * updating first.
 */
tooNew: boolean, 
/**
 * The target is a Switch version only in the cloud (the console itself):
 * the result is uploaded as its newest version, and the Switch gets it
 * at its next sync.
 */
cloudTarget: boolean, 
/**
 * The target is a game Steam installed whose saves Steam Cloud keeps
 * too (not taken over): Steam notices the new saves at its next sync
 * and may ask which to keep.
 */
steamCloud: boolean, };
