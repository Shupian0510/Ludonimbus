//! Game records: `library/<game id>.json`, one per card in the library
//! (docs/plan-v2.md, section 3.3).
//!
//! Contract of this module (nothing is here yet):
//!
//! - **Load**: every record with one listing of `library/` plus one read per
//!   record whose tag changed since the last time (a serializable cache maps
//!   file name to tag and record). A record with `mergedInto` is followed to
//!   the record that replaced it.
//! - **Save**: there is no conditional write, so read the record again,
//!   merge, add one to `rev`, write. Merge (pure): `ids`, `lines` (by id),
//!   `folders` and `tags` are unions, order kept; single values (`name`,
//!   `notes`, `rule`) come from the side with the higher `rev`; fields this
//!   version does not know are kept.
//! - **Identity** (pure): strong ids (`steam:`, `nx:`, `xbox:`, `gog:`,
//!   `epic:`, `ludusavi:`, `ra:`, console title ids, `rule:`) and the weak
//!   `name:<lowercase, no punctuation>`. Finding the record for a set of
//!   ids: a shared strong id is a match; a name alone is only a suggestion.
//! - **New records**: the id is a readable slug of the name plus four random
//!   characters (`stardew-valley-7k3f`; a name without Latin letters gets
//!   `game-...`), the first folder is the id, and there is one shared line
//!   `main`.
//! - **Duplicates** (pure): two devices created the same game. Records that
//!   share a strong id become one: the one created first stays, the other
//!   gets `mergedInto`, and its folders and lines move over. Two shared
//!   lines of the same format become one line; the path of the other goes
//!   into `also`.
//!
//! Requests: refreshing the library is 1 list + 1 read per changed record
//! (section 5.6).
