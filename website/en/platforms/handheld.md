# Handhelds and the controller view

On handhelds such as the ROG Ally, Legion Go and Steam Deck, Ludonimbus has two things made for a controller: full-screen questions when a game starts, and a view you can use with the controller alone.

Both are set up in Settings → "Handheld".

## Questions when a game starts

Starting a game from Steam sometimes needs a choice from you, such as a [conflict](../sync/conflicts.md), or ["Play with this PC's saves"](../sync/how-it-works.md#waiting) while newer saves are downloading. On a handheld these questions are full-screen windows: press the matching controller button, or move with the d-pad and press A.

"Questions when a game starts" has three choices:

| Choice | Meaning |
|---|---|
| Automatic | Full-screen when the game starts from Steam Big Picture or Game Mode, ordinary dialogs otherwise |
| Always full screen | |
| Always system dialogs | |

## The controller view

A full-screen library for the controller alone:

- **Library**: all your games with their sync status, including the ones that are only in the cloud.
- **Game page**: when it last synced, "Sync now", and the same [list of saves](../sync/versions.md) as on the desktop: pick one, A for "Make current", X for "Send to…". A replacement can be undone.
- **Conflicts**: choose "Use cloud saves" or "Use this PC's saves" right there.
- When another version has a newer save, it offers "Send to this version".

Adding games and connecting a cloud need typing and picking files, so they stay in the desktop view; the controller view's settings page has "Switch to the desktop view".

### Opening it

- Settings → "Handheld" → "Open the controller view".
- **From the Steam library**: click "Add to Steam" in Settings → "Handheld", and Ludonimbus appears in your Steam library as a non-Steam game (Steam has to be restarted). Started from Big Picture or Game Mode, it opens the controller view.
- **From the Xbox app** (Windows handhelds): "My Library → Add game", pick `ludonimbus-app.exe` in Ludonimbus's install folder and set the launch argument to `--handheld`.

::: tip
Games started from the Xbox app don't go through Steam, so their saves are only uploaded after the game exits, by Ludonimbus in the tray. To also pull the newest saves before playing, start the game from Steam.
:::

## Steam Deck

For installing on a Deck and the Decky plugin in Game Mode, see [Linux and Steam Deck](./linux.md).
