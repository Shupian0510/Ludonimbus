// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { NxDevice } from "./NxDevice";

export type NxStatus = { enabled: boolean, running: boolean, address: string | null, port: number, fingerprint: string | null, devices: Array<NxDevice>, pairingCode: string | null, error: string | null, };
