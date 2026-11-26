# Playing offline

When the cloud can't be reached (no internet, the NAS is off, OpenList isn't running…), you can still play.

## The cloud is unreachable before launch

When you start a game from Steam, Ludonimbus waits for the cloud for a moment at most (Settings → "This PC and sync" → "Wait for the cloud", **10 seconds** by default) and then starts the game, without a prompt, so you're never stuck waiting. That round syncs after exit instead.

If the pre-launch sync fails with an error (rather than just no connection), Ludonimbus shows the error and asks whether to start the game anyway; if you do, it tries to upload again after the game exits.

## Saves made offline

After the game exits, the new saves are queued on this PC, and the game page says how many are waiting to upload (uploaded once the cloud is reachable again).

They're uploaded automatically when the cloud is back: the tray app retries every so often, and they are also sent before the next launch from Steam (source "Uploaded before launch"). You can also click "Sync now".

![Game page: waiting to upload](/screenshots/en/game-detail-pending.png)

## A slow connection, an upload that isn't done

After the game exits, Ludonimbus waits for the upload for at most 10 seconds ("Wait for the upload"). After that the upload carries on in the background and Steam shows the game as stopped. The status on the game's Steam page then reads "Uploading in the background (3.2 MB)"; when nothing is uploading it reads "Waiting to upload" or "Couldn't reach the cloud; trying again later", and it returns to "up to date" when done.

Shutting down or sleeping before it's done loses nothing: the upload continues next time. To always wait until the upload is done, turn on "Always wait until it's done".

## What you'll see

- The library shows "Cloud offline".
- The game's Steam page shows that the cloud can't be reached, and how many versions are waiting.
- If the sidebar says the cloud needs reconnecting, it's not the network: the sign-in expired or the password changed. Reconnect in Settings → Cloud.

## Played offline on two devices

Both have saves the other doesn't, so the next sync shows a [conflict](./conflicts.md). Pick one; the other stays under [Saves](./versions.md).
