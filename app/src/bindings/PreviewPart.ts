// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { PartKind } from "./PartKind";

/**
 * What a backup would contain, by component (the app shows it before starting).
 */
export type PreviewPart = { kind: PartKind, name: string, version: string | null, files: number, bytes: number, excludedFiles: number, excludedBytes: number, };
