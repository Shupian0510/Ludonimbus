// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { NxUser } from "./NxUser";

export type NxDevice = { id: string, name: string, pairedAt: string, lastSeen: string | null, 
/**
 * Its user accounts, as it last reported them (empty until a Switch
 * app of protocol 3 connects).
 */
users: Array<NxUser>, };
