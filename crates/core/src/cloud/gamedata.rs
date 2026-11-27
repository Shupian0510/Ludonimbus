//! Where a game's playtime, achievements, screenshots and artwork are in the
//! cloud (docs/plan-v2.md, section 3.6). The one place that knows the rule;
//! [`crate::playtime`], [`crate::achievements::cloud`], [`crate::screenshots`]
//! and [`crate::artsync`] only ask it.
//!
//! These belong to the game, not to the key a game has on one device. A game
//! is its record, `library/<id>.json`, and the record lists the game's data
//! folders under `games/` ([`super::v2::GameRecord::folders`]): one for
//! almost every game, several when keys of 0.6 turned out to be one game.
//!
//! **Reading.** Every folder of the record, in the record's order. A game's
//! totals, unlocks and gallery are what all its folders hold together.
//!
//! **Writing.** Nothing is ever moved, and nothing is copied from one folder
//! to another.
//!
//! - One of the record's folders is named like this install's key: the
//!   install writes there, with the names 0.6 uses (`<device>.json`). An
//!   install keeps writing where it always did, and a device still on 0.6,
//!   which only knows `games/<its key>/…`, keeps seeing it.
//! - Otherwise (the install joined a record made under another key): the
//!   record's first folder, and what is kept per device carries the key in
//!   its name, `<device>~<key>.json` (a folder per device likewise), so two
//!   installs of one game on one device never write the same file.
//!
//! **Per-device files.** Each is one install's. The device it belongs to is
//! what the file says inside, else the part of its name before `~`; the
//! install is the part after `~`, else the folder the file is in. Readers
//! merge the files of one install (it may have written in two places when
//! the record's folders changed), add up installs, and group them by device
//! for display. A device still on 0.6 that lists such a folder sees a name
//! that isn't its own and takes the file for another device's, which gives
//! it the right totals too.
//!
//! **One copy per game** (artwork, achievement definitions): read from the
//! first folder that has it, in the record's order (artwork kind by kind);
//! written to this install's folder. What was read from another folder is
//! not written back: that would be a copy.
//!
//! **Which folders.** [`Folders::of`] asks the [`Cloud`]: no request once
//! this device has synced the game ([`Cloud::place`]), otherwise one listing
//! of `library/`. [`Folders::remembered`] never asks and uses what this
//! device last learnt; with nothing learnt, the record the game joined, else
//! its key, as in 0.6. Where the files go is decided when they are uploaded,
//! so everything that works without the cloud works as before.
//!
//! **Requests.** A game with one folder, named like the key, costs exactly
//! what 0.6 did. Each further folder costs one more listing (per-device
//! files) or one more read (one copy per game) per kind of data.

use super::client::Cloud;
use super::store::join;
use crate::config::GameEntry;
use crate::state;
use anyhow::Result;

/// Between the device and the install's key in a per-device name.
pub const SEP: char = '~';

/// A game's data folders, as one install (the game's key on this device)
/// reads and writes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    key: String,
    /// Never empty.
    folders: Vec<String>,
}

/// `games/<folder>/<kind>`.
pub fn dir(folder: &str, kind: &str) -> String {
    join(&["games", folder, kind])
}

/// The device and the install a per-device name stands for (`name` without
/// its extension, `folder` the game folder it is in).
pub fn owner<'a>(folder: &'a str, name: &'a str) -> (&'a str, &'a str) {
    match name.split_once(SEP) {
        Some((device, key)) if !device.is_empty() && !key.is_empty() => (device, key),
        _ => (name, folder),
    }
}

/// The device a per-device name (without its extension) belongs to.
pub fn device_of(name: &str) -> &str {
    owner("", name).0
}

impl Folders {
    /// The folders of the game `key` is an install of, in the record's
    /// order. None known: the key, as in 0.6.
    pub fn new(key: &str, folders: &[String]) -> Folders {
        let mut list: Vec<String> = Vec::new();
        for f in folders {
            if !f.is_empty() && !list.contains(f) {
                list.push(f.clone());
            }
        }
        if list.is_empty() {
            list.push(key.to_string());
        }
        Folders { key: key.to_string(), folders: list }
    }

    /// The game `key` in its own folder only, as 0.6 keeps it.
    pub fn of_key(key: &str) -> Folders {
        Folders::new(key, &[])
    }

    /// The folders of the game `key` as the cloud has them now
    /// ([`Cloud::game_folders`]).
    pub fn of(cloud: &Cloud, key: &str) -> Result<Folders> {
        Ok(Folders::new(key, &cloud.game_folders(key)?))
    }

    /// The folders this device last learnt for the game (with its save
    /// line), without asking the cloud. Nothing learnt: the folder of the
    /// record the game belongs to ([`GameEntry::record`], where a record's
    /// data begins), else its key. Never a folder under the key of a game
    /// that continues another game's card: that would show up as one more
    /// game in the cloud.
    pub fn remembered(game: &GameEntry) -> Folders {
        let mut known = state::game(&game.key).line.map(|l| l.folders).unwrap_or_default();
        if known.is_empty() {
            known.extend(game.record.iter().filter(|r| !r.is_empty()).cloned());
        }
        Folders::new(&game.key, &known)
    }

    /// The install's key: the game's id on this device.
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn all(&self) -> &[String] {
        &self.folders
    }

    /// The folder this install writes to: the one named like its key, else
    /// the record's first.
    pub fn own(&self) -> &str {
        match self.folders.iter().any(|f| *f == self.key) {
            true => &self.key,
            false => &self.folders[0],
        }
    }

    /// `games/<folder>/<kind>` of every folder, with the folder, in the
    /// record's order.
    pub fn read_dirs(&self, kind: &str) -> Vec<(&str, String)> {
        self.folders.iter().map(|f| (f.as_str(), dir(f, kind))).collect()
    }

    /// Where this install writes data of `kind`.
    pub fn write_dir(&self, kind: &str) -> String {
        dir(self.own(), kind)
    }

    /// The name of this install's per-device file (add the extension) or
    /// folder: the device's id in the folder named like the key, else
    /// `<device>~<key>`.
    pub fn own_name(&self, device: &str) -> String {
        match self.own() == self.key {
            true => device.to_string(),
            false => format!("{device}{SEP}{}", self.key),
        }
    }

    /// Whether the per-device name `name` (without its extension) in `folder`
    /// is this install's on `device`, wherever it was written.
    pub fn is_own(&self, folder: &str, name: &str, device: &str) -> bool {
        owner(folder, name) == (device, self.key.as_str())
    }

    /// A name for the file `name` of `folder` that no file of another of the
    /// game's folders has, for what this device keeps about it: the name
    /// itself in the folder named like the key (as 0.6 kept it),
    /// `<folder>+<name>` in the others.
    pub fn local_name(&self, folder: &str, name: &str) -> String {
        match folder == self.key {
            true => name.to_string(),
            false => format!("{folder}+{name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_install_writes_into_the_folder_named_like_its_key_as_0_6_did() {
        // Almost every game: one folder, the key.
        let f = Folders::of_key("celeste");
        assert_eq!(f.all(), ["celeste"]);
        assert_eq!(f.write_dir("playtime"), "games/celeste/playtime");
        assert_eq!(f.own_name("d-01"), "d-01");
        assert_eq!(f.local_name("celeste", "d-02.json"), "d-02.json");

        // Two keys of 0.6 that are one game: each install keeps its folder.
        let merged = folders(&["celeste", "celeste-xbox"]);
        let xbox = Folders::new("celeste-xbox", &merged);
        assert_eq!(xbox.own(), "celeste-xbox");
        assert_eq!(xbox.own_name("d-01"), "d-01");
        assert_eq!(
            xbox.read_dirs("playtime"),
            vec![("celeste", "games/celeste/playtime".to_string()), ("celeste-xbox", "games/celeste-xbox/playtime".to_string())]
        );
        assert_eq!(xbox.local_name("celeste", "d-02.json"), "celeste+d-02.json");
        assert_eq!(xbox.local_name("celeste-xbox", "d-02.json"), "d-02.json");
    }

    #[test]
    fn an_install_that_joined_another_record_names_its_files_after_its_key() {
        let f = Folders::new("celeste-gog", &folders(&["celeste", "celeste-xbox"]));
        assert_eq!(f.own(), "celeste");
        assert_eq!(f.write_dir("achievements"), "games/celeste/achievements");
        assert_eq!(f.own_name("d-01"), "d-01~celeste-gog");
        // The same device's other install of the game is not this one.
        assert!(f.is_own("celeste", "d-01~celeste-gog", "d-01"));
        assert!(!f.is_own("celeste", "d-01", "d-01"));
        assert!(!f.is_own("celeste", "d-01~celeste-steam", "d-01"));
        assert!(!f.is_own("celeste", "d-02~celeste-gog", "d-01"));
    }

    #[test]
    fn a_per_device_name_says_whose_it_is() {
        assert_eq!(owner("celeste", "d-01"), ("d-01", "celeste"));
        assert_eq!(owner("celeste", "d-01~celeste-gog"), ("d-01", "celeste-gog"));
        assert_eq!(device_of("d-01~celeste-gog"), "d-01");
        assert_eq!(device_of("d-01"), "d-01");
        // Not a name this rule makes: taken as it is.
        assert_eq!(owner("celeste", "~x"), ("~x", "celeste"));
        assert_eq!(owner("celeste", "d-01~"), ("d-01~", "celeste"));
    }

    #[test]
    fn folders_keep_the_records_order_once_each() {
        let f = Folders::new("b", &folders(&["a", "", "b", "a"]));
        assert_eq!(f.all(), ["a", "b"]);
        assert_eq!(Folders::new("k", &[]).all(), ["k"]);
    }
}
