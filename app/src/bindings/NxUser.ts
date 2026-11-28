// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * A user account on a paired Switch and how its saves are kept.
 */
export type NxUser = { uid: string, nickname: string, sync: boolean, 
/**
 * Kept apart from users of the same nickname on other Switches.
 */
separate: boolean, 
/**
 * The name of the save line its saves are on.
 */
line: string, };
