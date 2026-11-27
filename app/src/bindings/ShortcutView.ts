// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type ShortcutView = { cover: string | null, icon: string | null, account: string, appid: number, name: string, nameCorrupted: boolean, suggestedName: string, exe: string, startDir: string, launchOptions: string, adapter: string | null, adapterName: string | null, 
/**
 * One emulator game's saves (empty: the rule's own, or no rule).
 */
saves: Array<string>, addedKey: string | null, wrappedKey: string | null, };
