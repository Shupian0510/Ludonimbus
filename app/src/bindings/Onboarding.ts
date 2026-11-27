// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type Onboarding = { 
/**
 * When the guide was finished or closed; `None` = not seen yet.
 */
doneAt: string | null, 
/**
 * What the user said they use Ludonimbus for (emulators, recomps, steamThirdParty, multiDevice).
 */
usage: Array<string>, };
