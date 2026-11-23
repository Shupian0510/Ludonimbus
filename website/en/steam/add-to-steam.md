# Add to Steam

Games that aren't in Steam (Epic, GOG and Xbox app games, portable games, emulated games…) can be added to Steam in one click. The new shortcut syncs saves from the very first launch, and you get Steam's play time, screenshots and controller settings.

## Add

One at a time or several at once:

- **One game**: click "Add to Steam…" on the game page. The game program must be set first.
- **Several games**: after adding games in "Scan this PC", a banner says which of them aren't in Steam yet; click "Add to Steam…".

In the dialog:

1. Choose the **Steam account** (if you have several).
2. Turn **automatic artwork** on or off: Ludonimbus picks the top-rated cover, background, logo and icon from SteamGridDB. This needs a SteamGridDB API key in Settings; see [Artwork and SteamGridDB](./artwork.md).
3. Click "Add to Steam". If Steam is running it is closed briefly and reopened afterwards.

![Add to Steam](/screenshots/en/add-to-steam.png)

If some games couldn't be added, Ludonimbus says how many failed (the others are added) and shows the reason next to each one; click "Retry".

## Xbox app (PC Game Pass) games {#xbox}

An Xbox game's own program is protected by Windows and can't be started by other programs. Ludonimbus starts it through `gamelaunchhelper.exe` in the game folder, and uploads saves after the game itself exits.

If the game page says this is a protected Xbox app game, click "Use gamelaunchhelper.exe".

Xbox games usually have Xbox cloud saves already; if Ludusavi's database knows where the saves are, Ludonimbus keeps another copy with version history.

## Remove

For shortcuts made by Ludonimbus, click "Remove from Steam" on the game page. This deletes the shortcut and its artwork; the game, its saves and its entry in Ludonimbus are untouched, and you can add it again later.

Removing a game from Ludonimbus also deletes the Steam shortcut Ludonimbus made for it.
