---
title: Steam Integration and Cloud Save Plugin for Non-Steam Games
description: Ludonimbus plugs into Steam. Saves sync when you press Play, a Steam client plugin shows the cloud save status on the game's page, and non-Steam games can be added to Steam with SteamGridDB artwork in one click.
---

# Steam integration

Ludonimbus is built around Steam: you keep pressing **Play** in your Steam library, and non-Steam games get cloud saves that behave like Steam Cloud. There are three parts, and you can use any of them.

[Download for Windows](https://github.com/Shupian0510/Ludonimbus/releases/latest)

## 1. Sync when you press Play

Turn on **Steam auto-sync** on a game's page. Its Steam shortcut then starts `ludonimbus-run` first, which:

1. pulls newer saves from your cloud (or asks you if both sides changed);
2. starts the game and waits until the game and its launcher have exited;
3. uploads the new saves as a new version.

The shortcut's **name, icon, artwork and play time stay the same**, and Steam screenshots and controller settings work as before. Turning auto-sync off restores the original shortcut.

Steam overwrites shortcut changes made while it runs, so Ludonimbus closes Steam briefly, makes the change and reopens it. See [Steam auto-sync](../steam/auto-sync.md).

## 2. Cloud save status inside Steam

An optional Steam client plugin adds a **Ludonimbus cloud saves** item to the play bar of every game Ludonimbus manages, next to "Last played" and "Play time":

- it updates live: "up to date · 5 minutes ago", "game running, uploads after exit", "uploading saves…", "conflict, click to choose which to keep";
- the button on the right syncs now, and clicking the status opens the game in Ludonimbus;
- regular Steam games are not affected.

The plugin runs on [Millennium](https://github.com/SteamClientHomebrew/Millennium), an open-source plugin framework for the Steam client. Ludonimbus installs it in one click from Settings: it downloads the latest stable Millennium from its official GitHub releases, verifies its SHA-256 and installs it, the same steps as Millennium's own installer. Millennium is not an official Valve component, and the plugin can be removed in the same place.

![Settings: show cloud save status in Steam](/screenshots/en/settings-steam-plugin.png)

The plugin only displays information. It talks to the Ludonimbus tray app through a local interface that only listens on this PC and needs a token generated at every start, and it never touches your cloud account. See [Cloud status inside Steam](../steam/status-in-steam.md).

## 3. One-click Add to Steam, with artwork

Games that aren't in Steam yet, including Xbox app (PC Game Pass) games, can be added with one click or in bulk. Cloud saves work from their first launch.

- Cover, background, logo and icon are set automatically or picked from [SteamGridDB](https://www.steamgriddb.com/) (with your own free API key), or come from local images.
- You can choose which Steam account to add them to.

![Add to Steam with artwork](/screenshots/en/add-to-steam.png)

See [Add to Steam](../steam/add-to-steam.md) and [Artwork and SteamGridDB](../steam/artwork.md).

## A library that looks like Steam

The Ludonimbus window shows your games in a grid or list using the covers, banners and logos you set in Steam, and games that aren't in Steam can have artwork too. Games that Steam Cloud already syncs are recognised and listed apart, so nothing is managed twice.

## Good to know

- Ludonimbus runs on **Windows 10 / 11**. The Steam plugin appears in Steam's desktop mode, not in Big Picture.
- The plugin needs Ludonimbus running in the tray; without it, Steam and your games work as usual and auto-sync still runs.
- With several Steam accounts on one PC, pick the account in Settings → Steam.

## Questions

**Is this an official Steam Cloud feature?**
No. Ludonimbus is an independent open-source project. Saves go to your own storage, not to Valve's servers.

**Will Steam still count my play time?**
Yes. The shortcut is the same one, so play time, screenshots and controller settings are kept.

**Do I need the plugin for sync to work?**
No. The plugin only shows the status inside Steam. Sync is done by the shortcut and works without it.
