# Achievements

Achievements for emulated, recompiled and ported games: from the emulator's trophy records, your RetroAchievements records, or by reading the saves. Progress shows on the Steam game page, Steam's achievement popup appears when one unlocks in game, and records sync between your PCs.

![The Achievements tab of a game](/screenshots/en/game-achievements.png)

## Sources

| Source | Games | Notes |
|---|---|---|
| shadPS4, Lantern Engine | PS4 games | Reads the emulator's trophy records. Bloodborne on Lantern is linked automatically when Ludonimbus starts |
| RPCS3 | PS3 games | Reads RPCS3's trophy records |
| PS4 ports | PC games ported from the PS4 version | Reads the original game's trophy file in the port's folder |
| RetroAchievements | RetroArch, DuckStation, PCSX2, PPSSPP, Dolphin and other emulators with RetroAchievements | Connect your RetroAchievements account in Settings → Achievements |
| Save rules (TrophyNX) | Switch games on Ryujinx, Switch recomps, Breath of the Wild on Cemu | Tells achievements from the save. Rules come from the community project TrophyNX, downloaded at runtime and cached for offline use |
| Save rules (Ludonimbus) | Ocarina of Time on Longshot | Rules that come with Ludonimbus |

## Link a source

Open the Achievements tab on the game's page:

- **Automatic**: games with an obvious source (Bloodborne on Lantern, games with save rules) are linked when Ludonimbus starts. A source you unlinked is never linked again automatically.
- **Trophy data found**: sources Ludonimbus found on this PC; click Link.
- **RetroAchievements**: list the games you played recently and pick one, or enter the game ID (the number in the game page's address).
- **Choose a trophy file…**: RPCS3's `TROPUSR.DAT` / `TROPCONF.SFM`, shadPS4's trophy XML, or Lantern's `trophies.log`.

Linked sources can be refreshed or unlinked. Unlinking doesn't delete the records already in the cloud.

## In Steam

With the [Steam plugin](../steam/status-in-steam.md), achievements appear where Steam shows them for its own games: "Achievements x/y" with a progress bar in the play bar, and an Achievements section at the top of the right column. Click either for the full list; locked ones are greyed out.

### Unlock popups

When an achievement unlocks in game, Steam's own achievement popup appears (same look and sound, over the game through the Steam overlay). When Steam isn't running or can't show it, a small Ludonimbus window can be used instead; it's only visible over windowed or borderless fullscreen games.

- Settings → Achievements has "Test popup" and "Test in 8 seconds" (switch to the game).
- Each game's Achievements tab can turn its popups off.
- If the emulator shows its own notification too, turn that one off in the emulator so you don't get two (the Achievements tab has hints per emulator).
- Ludonimbus needs to be running in the tray.

## Several PCs

Each PC writes only its own record, `Ludonimbus/games/<game>/achievements/<device>.json` in the cloud. Achievements unlocked on another PC show up too, marked "Unlocked on …"; the earliest unlock time wins.

## PS4 trophy names {#ps4-trophy-key}

PS4 trophy names and descriptions are in an encrypted file that comes with the game. Reading it needs the **PS4 trophy key** (the same one shadPS4 uses). Ludonimbus doesn't include it:

- if `TrophyKey` is set in shadPS4's settings, it's used automatically;
- otherwise enter it in Settings → Achievements → PS4 trophy key (32 hexadecimal characters). It's stored only on this PC and never uploaded.

Without a key, PS4 trophies show as numbers and icons; progress, popups and sync work the same.

![Settings → Achievements](/screenshots/en/settings-achievements.png)
