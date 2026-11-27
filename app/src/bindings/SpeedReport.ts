// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SpeedPhase } from "./SpeedPhase";
import type { Verdict } from "./Verdict";

export type SpeedReport = { kind: string, label: string, objectBytes: number, totalBytes: number, 
/**
 * Average time of one small write and one small read.
 */
smallWriteMs: number, smallReadMs: number, uploadSerial: SpeedPhase | null, uploadParallel: SpeedPhase | null, downloadSerial: SpeedPhase | null, downloadParallel: SpeedPhase | null, 
/**
 * Best measured speeds, bytes per second.
 */
uploadBps: number, downloadBps: number, verdict: Verdict, 
/**
 * Test files that could not be deleted.
 */
cleanupError: string | null, 
/**
 * What is known about this kind of cloud (quotas, limits).
 */
notes: Array<string>, };
