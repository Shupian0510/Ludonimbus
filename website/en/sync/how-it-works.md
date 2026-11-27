# How sync works

Ludonimbus syncs the way Steam Cloud does: **pull before launch, upload after exit**. Every upload leaves a save in the cloud. Once written it is never changed; it records the device, the time and which save it follows.

The rule is: **playing never waits for the cloud, unless there really is newer progress to fetch.**

## When it syncs

| When | What happens |
|---|---|
| Before a game starts from Steam | Newer saves in the cloud are pulled; saves that weren't uploaded yet are uploaded first |
| After the game and its launcher exit | If the saves changed, they are uploaded |
| A game started outside Steam | With Ludonimbus in the tray, it notices the game exiting and uploads |
| You press "Stop" in Steam | The tray app notices and uploads right away |
| You click "Sync now" | On the game page, or the sync button on the game's Steam page |
| In the background | The tray app re-checks the cloud from time to time and finishes uploads that weren't done; the library updates |

Only one sync runs for a game at a time: the app, the tray watcher, the small program that starts the game from Steam and the sync button in Steam take turns.

## How long it waits {#waiting}

**Before launch** it waits for the cloud for at most **10 seconds** (Settings → "This PC and sync" → "Wait for the cloud"):

- No newer saves in the cloud: usually one request settles it, and the game starts.
- Time is up and the cloud hasn't answered: the game starts, and this round syncs after exit instead.
- Time is up while newer saves are downloading: the download goes on, and a small window says how much there is, with a "Play with this PC's saves" button. If you click it, both are kept and you're asked later which one to use.

**After exit** it waits for the upload for at most **10 seconds** ("Wait for the upload"). If the upload isn't done by then, it carries on in the background and Steam shows the game as stopped, so you aren't kept waiting. With "Always wait until it's done" turned on it waits to the end, which suits devices you shut down or put to sleep right after playing.

Shutting down before the upload is done is fine: the saves are queued on this PC and the upload continues next time. An upload cut off halfway never leaves half a save in the cloud.

## How it decides

Every save records how far it includes each device's changes. Ludonimbus tells newer from older by that, **not by the clock**: PCs with a wrong time or different time zones make no difference.

| This PC | Cloud | Result |
|---|---|---|
| Unchanged | Nothing newer | Nothing to do |
| Unchanged | Newer saves | Pull them (keeping this PC's saves first) |
| Changed | Nothing newer | Upload |
| Changed | Newer saves | **Conflict**: both are kept and you choose. See [Conflicts](./conflicts.md) |
| No saves | Has saves | Pull (for example after reinstalling) |
| New device, first sync | Has saves | Same content counts as synced, otherwise it's a conflict |
| Cloud unreachable | — | Start anyway and queue the saves for later. See [Playing offline](./offline.md) |

A sync that wasn't waited for ("start anyway" above) can at worst become a conflict: it is noticed and you're asked. Nothing is overwritten silently.

Apart from cleaning up old automatic saves by your settings, Ludonimbus never deletes a save. Kept saves, the newest save and the save each device last synced are never cleaned up. See [Saves](./versions.md).

## Statuses in the library

A dot under each game's cover shows its status; hover for the name. Statuses that need you (a conflict, an error) are written out next to the dot.

| Status | Meaning |
|---|---|
| Synced | This PC and the cloud match |
| Ready to upload | This PC has new saves that aren't uploaded yet |
| Newer in the cloud | Another device uploaded newer saves; they are pulled before the next launch |
| Can restore from cloud | No saves on this PC, but the cloud has some |
| Not uploaded yet | Saves on this PC, nothing in the cloud yet |
| Save conflict | Both sides changed; you need to choose |
| Cloud offline | The cloud can't be reached right now |
| No cloud connected | No cloud is connected in Settings |
| No saves yet | No saves on this PC or in the cloud yet |
| Game running | The game is running; it syncs after exit |

## Waiting for the game to really exit

Some games open a launcher that then opens the game, or keep writing saves for a moment after closing. Ludonimbus:

- waits until the game **and its launcher** have exited; built-in rules list the game's real process name, so the game is still recognised as running when the launcher exits first;
- waits for a lock file to be released when a game leaves one while writing saves (for at most 20 s);
- can upload as soon as the game process exits for launchers that stay open; see [Steam auto-sync](../steam/auto-sync.md#launcher).

## Writing into the game

Putting a save into the game's save folder (a pull, or "Make current") happens as a whole: each file is first written next to its place and checked, and only when all of them are right are they swapped in. If something fails or the power goes out halfway, the saves are either the new ones or exactly what they were, never half of each.

Before the swap, saves on this PC that weren't uploaded yet are kept; a wrong swap can be [undone](./versions.md#make-current).

## Several versions of one game

The Steam version, the Xbox version and one in an emulator share one progress automatically when their saves have the same format, see [Several versions of one game](./game-versions.md). For games bought on Steam, see [Games Steam installed](../steam/steam-cloud.md).

To look at saves, keep them and clean them up, see [Saves](./versions.md).
