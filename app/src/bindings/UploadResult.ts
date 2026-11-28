// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { Outcome } from "./Outcome";
import type { StatusReport } from "./StatusReport";

/**
 * What 「全部同步」 did with one game: the saves here uploaded, nothing
 * written into them ([`sync::upload`]), and how the game stands now.
 */
export type UploadResult = { outcome: Outcome, check: StatusReport, };
