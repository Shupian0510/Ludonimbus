//! Content packs: where the bytes of a snapshot are (docs/plan-v2.md,
//! section 3.4).
//!
//! A pack is an ordinary zip: one entry `files/<canonical path>` per file,
//! plus `snapshot.json`, a copy of the manifest it was written for. A
//! snapshot's manifest names, per file, the pack that holds it
//! ([`super::v2::FileRef::pack`]; none = the pack named like the snapshot).
//! Packs belong to the game, not to a save line:
//! `games/<folder>/packs/<pack id>.zip`. A snapshot of the 0.6 layout
//! (`v` < 2) has its one pack beside its manifest: `<line dir>/<id>.zip`.
//!
//! Contract of this module (nothing is here yet):
//!
//! - **Plan** (pure): given the snapshot id, the local files (canonical
//!   path, size, hash, mtime, where the bytes are on disk), the manifest
//!   this device synced last (the base, if any) and a policy, decide which
//!   files go into which new pack and which are referenced from the packs of
//!   the base. Policy defaults: saves of at most 16 MiB always get one
//!   complete pack; larger ones only pack files whose path + hash differ
//!   from the base; more than 64 MiB of new content is split into `<id>`,
//!   `<id>-2`, ... with every file whole in one pack. Only packs the base
//!   itself uses may be referenced.
//! - **Write**: build the zips locally, upload them, and only then let the
//!   caller write the manifest ([`super::line::write_manifest`]). After the
//!   manifest is written, list `packs/` once and upload a complete pack
//!   again when a referenced one is gone.
//! - **Read**: fetch the packs a manifest needs (all files, or a chosen
//!   subset), looking in every folder of the game, extract them to a folder
//!   and check every file against the size and hash in the manifest. A
//!   missing pack is its own kind of error: the caller reports "this version
//!   is incomplete" and leaves the local saves alone.
//! - **Clean up** (pure): given the listing of `packs/` and the packs every
//!   remaining manifest uses (trash included), name the packs nothing refers
//!   to. A pack without a manifest may be an upload in progress: only packs
//!   older than a grace period are named (a pack id starts with a ULID,
//!   which carries its time).
//!
//! Sizes and thresholds are the policy of whoever writes, not part of the
//! format: a reader only needs to follow `in`.
