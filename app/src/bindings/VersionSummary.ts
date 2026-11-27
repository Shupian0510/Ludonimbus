// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { PartSummary } from "./PartSummary";

export type VersionSummary = { format: number, id: string, game: string, name: string, createdAt: string, deviceId: string, deviceName: string, appVersion: string, components: Array<PartSummary>, totalSize: number, storedSize: number, newBytes: number, fileCount: number, 
/**
 * BLAKE3 of the `.manifest` file as stored.
 */
manifestHash: string, manifestSize: number, comment: string, };
