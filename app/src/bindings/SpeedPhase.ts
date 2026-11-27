// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type SpeedPhase = { threads: number, bytes: number, objects: number, secs: number, bytesPerSec: number, 
/**
 * Stopped at the time limit before all the data was moved.
 */
capped: boolean, error: string | null, };
