# Several versions of one game

You may own a game more than once: the Steam version and the Xbox app's version, a PC version and one in an emulator, a copy on each of two PCs. Ludonimbus puts them on **one card**, and versions whose saves have the same format **share one progress automatically**.

## The card and the version strip

A game has one card in the library. The row under the cover shows the logos of the platforms it has saves for; the ones that aren't on this PC are greyed out.

On the game's page, the **version strip** shows one ring per progress, listing the "platform · device" pairs that share it. Versions in the same ring share one progress; separate rings are kept apart.

Click "This PC" to change how this device's version is stored:

| Menu item | What it does |
|---|---|
| Share with the other devices / Keep this device's own / Follow "…" | See [Multiple devices](./devices.md#independent) |
| Sync automatically / Only when I ask | Whether to sync when the game starts and exits. Games Steam installed also have "Backup only", see [Games Steam installed](../steam/steam-cloud.md) |
| Change the save location… | Opens the Settings tab to change where the saves are |
| Keep apart from the other versions | This version goes back to its own progress |
| Make it a game of its own | Takes it off this card, as a card of its own |

When two cards are really the same game, click "Link another version…" on the version strip to put them on one card.

## When versions share automatically

When you add a game, Ludonimbus first works out which game it is, then looks in the cloud for a progress of that game with the same save format:

- **Shared without asking**: the same game on the same platform on several devices; the same Switch game on a console and in the emulators; PC versions that keep their saves in the same place.
- **Asked once**: the same game, with its saves in a different place, such as another store's version.
- **Asked once, by a rule**: for games in the rule list (such as Stardew Valley), the Switch version (on a console or in an emulator) and the PC version keep the same files, see ["The Switch and PC versions"](#rule) below.
- **Kept apart**: versions with different save formats (a PC version and a console version, with no rule saying they're interchangeable). They are still on one card, each with its own progress, and you can move saves by hand with ["Send to…"](#send).

Games that only have the same name, and can't be told to be the same game, are not merged.

## "Share the progress?" {#ask-once}

When Ludonimbus needs to ask, a line appears under the version strip:

> These are the same saves as the version on …. Share the progress?  [Share] [They aren't]

The "?" next to it says how many files match and which folders correspond.

- **Share**: this version joins that progress. If both have the same saves, they simply count as synced; if this device has an older save, the newest is fetched; if both have progress of their own, you choose which to keep, as in a [conflict](./conflicts.md), and the other stays under Saves.
- **They aren't**: it isn't suggested again, and the two stay apart.

Once confirmed on one device, the same version on your other devices joins without asking.

Nothing is asked while this version has no saves on this PC yet: play it once and save first.

To go back, click "Keep apart from the other versions" in the "This PC" menu of the version strip.

## The Switch and PC versions {#rule}

Some games keep the same files on the Switch and on PC, and Ludonimbus's rule list says which games and where their saves are. For these games, the Switch version (in an emulator or on a console) and the PC version can share one progress.

- You're asked the first time: "The PC version keeps the same saves as this one. Share the progress?" (on the PC version, it names the Switch version). The "?" says which game's rule it is and how many files match.
- Only the save files the rule names are shared. Each platform's own files, such as settings, stay where they are and aren't replaced.
- An older version of a game can't read a newer save. A save made by a newer version of the game on the other platform isn't brought over by itself: the game page says so; update the game to the same version, then make the save current on the Saves tab.

A console's version syncs through a PC, see [Switch console](../platforms/switch.md).

## The Xbox app's version (experimental) {#xbox}

Games installed by the Xbox app (Game Pass) keep their saves in a set of "container" files, named differently from the Steam version's files. Ludonimbus can turn the containers back into ordinary files, so the Xbox version can share progress with the Steam version:

- **After playing the Xbox version**, the restored files are uploaded, and the Steam version on your other devices picks them up as usual.
- **Writing into the Xbox version is experimental and never happens by itself.** When the cloud has newer progress, the Xbox version shows "Newer in the cloud" but nothing is written. It is written only when you click "Make current" or choose "Use cloud saves" in a conflict; you're told before it happens, and the saves here are kept first.
- Close the game and the Xbox app before writing. Afterwards, go offline, start the game once and save, then go online; if Xbox asks which copy to keep, choose this PC's. Xbox's own cloud sync may put its copy back.

## Send to… {#send}

Two versions with different save formats can't share, but you can move saves once by hand: "More → Send to…" on a row of the Saves tab, then pick another version on this PC.

The dialog says how it will be written this time:

- Same format: the target's saves are replaced by this one, the same as "Make current".
- Matched by file name: the save files go into the target's save folder; files with the same name are overwritten, new ones are added, and the target's other files are left alone.
- The game is in the rule list (for example Stardew Valley on Switch and PC): only the saves are moved, by the rule, and settings files aren't touched. If the save was written by a newer version of the game than the target has, you're warned: an older game can't read a newer save, so update the game first.
- Different platforms and no rule: you're warned that the files usually can't be used as they are.

Before anything is written, the target's current saves are kept; you can make them current again from its Saves tab.

When another version has a newer save, a line at the top of the game page says so, and "Send to this version…" opens this dialog directly.
