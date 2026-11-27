// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { CloudOnlyView } from "./CloudOnlyView";

export type CloudLibrary = { cloudOnly: Array<CloudOnlyView>, 
/**
 * key → family for every game (this PC's and the cloud's).
 */
families: { [key in string]: string }, fetchedAt: string | null, 
/**
 * Why the list couldn't be fetched (the cached one is returned).
 */
error: string | null, };
