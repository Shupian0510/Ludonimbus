//! Version vectors: which devices' changes a snapshot already contains.
//!
//! Every snapshot of a save line carries one. A snapshot made on device `d`
//! takes the vector of the version its saves were based on and adds one to
//! `d`'s entry. "Newest" and "forked" then follow from comparing vectors,
//! never from clocks (docs/plan-v2.md, section 5.1).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Pseudo device that orders the snapshots of the 0.6 layout, which have no
/// vector of their own (docs/plan-v2.md, section 5.3).
pub const LEGACY: &str = "legacy";

/// `{device id: how many of that device's changes are included}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Vv(BTreeMap<String, u64>);

/// How one version relates to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Equal,
    /// Contains everything the other has, and more.
    Newer,
    Older,
    /// Each has changes the other lacks.
    Forked,
}

impl Vv {
    pub fn new() -> Self {
        Vv::default()
    }

    /// The vector given to the `n`th (1-based) snapshot of a 0.6 folder.
    pub fn legacy(n: u64) -> Self {
        let mut v = Vv::default();
        if n > 0 {
            v.0.insert(LEGACY.to_string(), n);
        }
        v
    }

    pub fn get(&self, device: &str) -> u64 {
        self.0.get(device).copied().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.0.values().all(|&n| n == 0)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, u64)> + '_ {
        self.0.iter().map(|(d, n)| (d.as_str(), *n))
    }

    /// `self` includes every change `other` includes.
    pub fn contains(&self, other: &Vv) -> bool {
        other.0.iter().all(|(d, &n)| self.get(d) >= n)
    }

    pub fn compare(&self, other: &Vv) -> Order {
        match (self.contains(other), other.contains(self)) {
            (true, true) => Order::Equal,
            (true, false) => Order::Newer,
            (false, true) => Order::Older,
            (false, false) => Order::Forked,
        }
    }

    /// Take the larger count of every device.
    pub fn join(&mut self, other: &Vv) {
        for (d, &n) in &other.0 {
            if n > self.get(d) {
                self.0.insert(d.clone(), n);
            }
        }
    }

    /// The vector of a new snapshot made on `device` that replaces `bases`
    /// (one base normally; every head when it settles a fork). `floor` is the
    /// highest count this device has used on the line so far, so a count is
    /// never handed out twice even when the bases don't include it.
    pub fn next<'a>(bases: impl IntoIterator<Item = &'a Vv>, device: &str, floor: u64) -> Vv {
        let mut v = Vv::default();
        for b in bases {
            v.join(b);
        }
        let n = v.get(device).max(floor) + 1;
        v.0.insert(device.to_string(), n);
        v
    }
}

/// The items nothing else contains: one means "the newest version", several
/// mean a fork. Returns their positions in `items`.
pub fn heads<T>(items: &[T], vv: impl Fn(&T) -> &Vv) -> Vec<usize> {
    (0..items.len())
        .filter(|&i| {
            let mine = vv(&items[i]);
            !(0..items.len()).any(|j| {
                j != i
                    && match vv(&items[j]).compare(mine) {
                        Order::Newer => true,
                        // The same vector twice: the later item stands for both.
                        Order::Equal => j > i,
                        Order::Older | Order::Forked => false,
                    }
            })
        })
        .collect()
}

/// The highest count `device` has in any of `vectors`: its counter on a save
/// line, recovered from the cloud when the local state was lost.
pub fn max_count<'a>(vectors: impl IntoIterator<Item = &'a Vv>, device: &str) -> u64 {
    vectors.into_iter().map(|v| v.get(device)).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vv(pairs: &[(&str, u64)]) -> Vv {
        Vv(pairs.iter().map(|(d, n)| (d.to_string(), *n)).collect())
    }

    #[test]
    fn contains_and_compare() {
        let p = vv(&[("a", 5), ("b", 3)]);
        let newer = vv(&[("a", 6), ("b", 3)]);
        assert!(newer.contains(&p));
        assert!(!p.contains(&newer));
        assert_eq!(newer.compare(&p), Order::Newer);
        assert_eq!(p.compare(&newer), Order::Older);
        assert_eq!(p.compare(&p.clone()), Order::Equal);
        // A zero entry is the same as no entry.
        assert_eq!(vv(&[("a", 1), ("c", 0)]).compare(&vv(&[("a", 1)])), Order::Equal);
        assert!(Vv::new().is_empty());
        assert!(p.contains(&Vv::new()));
    }

    /// The example of docs/plan-v2.md section 1: A plays offline, B plays
    /// online, both starting from the same version.
    #[test]
    fn offline_play_on_two_devices_is_a_fork() {
        let p = vv(&[("a", 5), ("b", 3)]);
        let s_a = Vv::next([&p], "a", 0);
        let s_b = Vv::next([&p], "b", 0);
        assert_eq!(s_a, vv(&[("a", 6), ("b", 3)]));
        assert_eq!(s_b, vv(&[("a", 5), ("b", 4)]));
        assert_eq!(s_a.compare(&s_b), Order::Forked);

        let all = [p.clone(), s_a.clone(), s_b.clone()];
        assert_eq!(heads(&all, |v| v), vec![1, 2]);

        // Settling it: the new version contains both sides.
        let merged = Vv::next([&s_a, &s_b], "a", 0);
        assert_eq!(merged, vv(&[("a", 7), ("b", 4)]));
        let all = [p, s_a, s_b, merged];
        assert_eq!(heads(&all, |v| v), vec![3]);
    }

    #[test]
    fn heads_survive_pruning_the_middle() {
        let mut chain = vec![Vv::next([&Vv::new()], "a", 0)];
        for i in 1..6 {
            let dev = if i % 2 == 0 { "a" } else { "b" };
            chain.push(Vv::next([&chain[i - 1]], dev, 0));
        }
        assert_eq!(heads(&chain, |v| v), vec![5]);
        // Retention removed everything but the first and the last.
        let pruned = [chain[0].clone(), chain[5].clone()];
        assert_eq!(heads(&pruned, |v| v), vec![1]);
    }

    #[test]
    fn equal_vectors_give_one_head() {
        let v = vv(&[("a", 2)]);
        let all = [v.clone(), v.clone()];
        assert_eq!(heads(&all, |v| v), vec![1]);
        assert!(heads::<Vv>(&[], |v| v).is_empty());
    }

    #[test]
    fn legacy_snapshots_keep_their_order_and_v2_builds_on_them() {
        let old: Vec<Vv> = (1..=3).map(Vv::legacy).collect();
        assert_eq!(heads(&old, |v| v), vec![2]);
        let first_v2 = Vv::next([&old[2]], "a", 0);
        assert_eq!(first_v2, vv(&[("a", 1), (LEGACY, 3)]));
        assert_eq!(first_v2.compare(&old[0]), Order::Newer);
    }

    #[test]
    fn counter_never_repeats() {
        // The device's last upload isn't part of the base (state was lost and
        // an older version restored): the floor keeps the count unique.
        let base = vv(&[("a", 2), ("b", 1)]);
        assert_eq!(Vv::next([&base], "a", 6).get("a"), 7);
        let seen = [vv(&[("a", 6)]), vv(&[("a", 3), ("b", 9)])];
        assert_eq!(max_count(&seen, "a"), 6);
        assert_eq!(max_count(&seen, "c"), 0);
    }

    #[test]
    fn serializes_as_a_plain_object() {
        let v = vv(&[("d-1", 12), ("d-2", 4)]);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, r#"{"d-1":12,"d-2":4}"#);
        assert_eq!(serde_json::from_str::<Vv>(&json).unwrap(), v);
    }
}
