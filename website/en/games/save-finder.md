# Save finder

When Ludonimbus doesn't know where a game keeps its saves, the save finder opens: click "Choose saves…" in "Scan this PC", or pick a game program in "Add manually".

It offers four ways, tried top to bottom; any one that finds the saves is enough.

![Save finder](/screenshots/en/save-finder.png)

## 1. Search Ludusavi's database

Ludusavi's database knows where thousands of PC games keep their saves (from PCGamingWiki). Search by the game's English name and pick the result; no need to choose folders yourself.

## 2. Common save locations

Ludonimbus looks in Documents, Saved Games, AppData and the game folder for folders named like the game, most recently changed first. Each candidate shows its file count, size, last change and a few example file names. Tick the one that looks right.

## 3. Play and find

For games nothing else recognises, this is the most reliable way:

1. Click "Start recording and launch the game" (without a game program it's "Start recording"; then open the game yourself).
2. Play for a bit, **save once**, then quit the game.
3. Click "Done".

Ludonimbus lists the folders that were written to in the meantime (caches, logs and the like are left out); the first one is usually the save folder. If it found no clear change, try again and make sure the game really saved, or use option 4.

## 4. Choose a folder yourself

Click "Choose folder…" and pick the save folder. The quickest way when you already know where it is.

## Then

The bottom shows "N save folders selected" or "Using Ludusavi's database: …". Check the name and click "Add".

::: warning Don't pick the whole game folder
If the selected folders hold more than 200 MB, Ludonimbus warns that you may have picked the whole game folder. Saves are usually a few KB to tens of MB; treating the whole game folder as saves would upload the entire game on every sync.
:::

You can change the save locations and excluded files later on the game page's Settings tab; see [Settings](../reference/settings.md#per-game).
