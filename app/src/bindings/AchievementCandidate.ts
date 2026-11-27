// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { Binding } from "./Binding";

export type AchievementCandidate = { binding: Binding, 
/**
 * Game title from the trophy files, when known.
 */
title: string | null, 
/**
 * Where the files are.
 */
location: string, 
/**
 * The only sensible choice (e.g. Bloodborne on Lantern): link it without asking.
 */
sure: boolean, };
