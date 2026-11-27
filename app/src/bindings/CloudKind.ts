// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * Which kind of cloud a [`CloudConfig`] is: the `kind` it has in JSON
 * (and [`crate::cloud::kind_id`]).
 */
export type CloudKind = "none" | "localFolder" | "googleDrive" | "webDav" | "s3" | "baiduPan" | "quark";
