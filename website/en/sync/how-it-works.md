# How sync works

Ludonimbus syncs the way Steam Cloud does: **pull before launch, upload after exit**. The cloud holds versions, and each version is an append-only zip snapshot that records the device, the time and the previous version.

## When it syncs

| When | What happens |
|---|---|
| Before a game starts from Steam | Newer versions in the cloud are pulled; versions that weren't uploaded yet are uploaded first |
| After the game and its launcher exit | If the saves changed, a new version is uploaded |
| A game started outside Steam | With Ludonimbus in the tray, it notices the game exiting and backs it up |
| You press "Stop" in Steam | The tray app notices and uploads right away |
| You click "Sync now" | On the game page, or the sync button on the game's Steam page |
| In the background | Every game is re-checked against the cloud every 10 minutes and the library updates |

The same game is never synced twice at once: the app, the tray watcher and the sync button in Steam share one lock.

## How it decides

Each sync compares "did this PC's saves change since the last sync?" with "is there a new version in the cloud since the last sync?":

| This PC | Cloud | Result |
|---|---|---|
| Unchanged | No new version | Nothing to do |
| Unchanged | New version | Pull the cloud version (protecting the local saves first) |
| Changed | No new version | Upload |
| Changed | New version | **Conflict**: both are kept and you choose. See [Conflicts](./conflicts.md) |
| No saves | Has versions | Pull (for example after reinstalling) |
| New PC, first sync | Has versions | Same content counts as synced, otherwise it's a conflict |
| Cloud unreachable | — | Start anyway and queue uploads for later. See [Playing offline](./offline.md) |

Apart from cleaning up old versions by your retention settings, Ludonimbus never deletes a version, and the clean-up never touches manual backups, locked versions, the newest version or the version each PC last synced.

## Statuses in the library

| Status | Meaning |
|---|---|
| Synced | This PC and the cloud match |
| To upload | This PC has new saves that aren't uploaded yet |
| Cloud is newer | Another PC uploaded newer saves; they are pulled before the next launch |
| Can restore from cloud | No saves on this PC, but the cloud has some |
| Not uploaded | Saves on this PC, nothing in the cloud yet |
| Conflict | Both sides changed; you need to choose |
| Cloud offline | The cloud can't be reached right now |
| No cloud | No cloud is connected in Settings |
| No saves | No saves on this PC or in the cloud yet |
| Game running | The game is running; it syncs after exit |

## Waiting for the game to really exit

Some games open a launcher that then opens the game, or keep writing saves for a moment after closing. Ludonimbus:

- waits until the game **and its launcher** have exited; built-in rules list the game's real process name, so the game is still recognised as running when the launcher exits first;
- waits for a lock file to be released when a game leaves one while writing saves (for at most 20 s);
- can upload as soon as the game process exits for launchers that stay open; see [Steam auto-sync](../steam/auto-sync.md#launcher).

## What's in a version

Each version is a zip with the save files themselves plus a description: which device, when, why it was uploaded (for example "auto after exit" or "manual backup") and which version came before. Paths are stored relative to the game folder, so saves go back to the right place even after moving the game or switching PCs.

To view, restore and clean up versions, see [Version history](./versions.md).
