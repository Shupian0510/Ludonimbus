// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SnapshotSummary } from "./SnapshotSummary";

export type Outcome = { "kind": "upToDate" } | { "kind": "uploaded", id: string, } | { "kind": "queued", id: string, reason: string, } | { "kind": "pulled", id: string, device: string, createdAt: string, } | { "kind": "conflict", localFiles: number, localSize: number, cloud: SnapshotSummary, } | { "kind": "nothingToSave" } | { "kind": "noCloud" } | { "kind": "offline", message: string, };
