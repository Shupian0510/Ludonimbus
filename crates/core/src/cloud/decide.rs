//! What a sync should do (docs/plan-v2.md, section 5.2): one pure function
//! from "what this device has" and "what the save line looks like" to an
//! action. No clocks: only version vectors and content hashes.
//!
//! Contract of this module (nothing is here yet).
//!
//! Input: whether there are saves on this device, their content hash,
//! whether they changed since the last sync, the snapshot this device
//! synced last (id, vector, content hash); and a [`super::line::LineView`].
//!
//! | saves here | the line | action |
//! |---|---|---|
//! | unchanged | the head is what was synced last | nothing |
//! | unchanged | one head that contains what was synced last | pull it |
//! | changed | the head is what was synced last | push |
//! | changed | one head, not what was synced last | conflict: ask |
//! | any | several heads | same content everywhere: no conflict; otherwise ask which one (the saves here are a candidate when they changed) |
//! | none | any snapshot | pull |
//! | any | empty | push (first upload) |
//!
//! A push says which vectors the new snapshot is built on (see
//! [`crate::vv::Vv::next`]): the base normally, every head when it settles
//! a fork. Saves here that are identical to the head (same content hash)
//! never cause a transfer or a question, whatever the vectors say.
