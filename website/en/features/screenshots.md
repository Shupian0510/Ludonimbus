# Screenshot sync

Screenshots you take in Steam (F12 by default) are uploaded to your cloud; with "upload and download", screenshots from your other PCs show up on this one too. It works for games linked to a Steam shortcut.

![The Screenshots tab of a game](/screenshots/en/game-screenshots.png)

## Turn it on

Screenshot sync is **off by default**, because screenshots are much bigger than saves (1–2 MB per 4K screenshot). Choose in Settings → Steam → Screenshot sync, or on a game's Screenshots tab:

| Option | What it does |
|---|---|
| Off | Default. Screenshots stay on this PC |
| Upload this PC's screenshots | Screenshots taken here go to your cloud |
| Upload, and download other PCs' screenshots | Also puts screenshots from your other PCs into this PC's Steam screenshot folder of the same game |

## When it syncs

- About 15 seconds after a game exits (the saves upload first);
- 1 minute after Ludonimbus starts, then every 30 minutes;
- When you press "Sync screenshots now" on the Screenshots tab.

Nothing syncs while the game is running.

## Good to know

- Screenshots are uploaded with the thumbnails Steam makes, to `Ludonimbus/games/<game>/screenshots/<device>/`. They're not part of the save version history and are never cleaned up automatically.
- Downloaded screenshots go into Steam's screenshot folder and appear in Steam's screenshot manager **after Steam is reopened**. The shortcut's number can differ between PCs; Ludonimbus uses this PC's folder.
- A screenshot you delete in Steam isn't downloaded again, and screenshots downloaded from other PCs aren't uploaded again as this PC's.
- The Screenshots tab shows screenshots on this PC and in the cloud: "This PC", "from …", "Not uploaded" and "Cloud only" tell you where each one is. Click one for the full image; "Show in folder" opens its folder. The space they take in the cloud is shown at the top.
