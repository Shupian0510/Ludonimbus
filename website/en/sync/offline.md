# Playing offline

When the cloud can't be reached (no internet, the NAS is off, OpenList isn't running…), you can still play.

## The cloud is unreachable before launch

When you start a game from Steam, Ludonimbus waits briefly (Settings → "This PC and sync" → "Network timeout", **8 seconds** by default) and then starts the game anyway, so you're never stuck waiting.

If the pre-launch sync fails with an error (rather than just no connection), Ludonimbus shows the error and asks whether to start the game anyway; if you do, it tries to upload again after the game exits.

## Saves made offline

After the game exits, the new saves are kept on this PC as versions, and the game page says how many are waiting to upload (uploaded automatically once the cloud is reachable).

They're uploaded automatically when the cloud is back: before the next launch from Steam (trigger "Uploaded before launch") or when the tray app re-checks. You can also click "Sync now".

![Game page: waiting to upload](/screenshots/en/game-detail-pending.png)

## What you'll see

- The library shows "Cloud offline".
- The game's Steam page shows that the cloud can't be reached, and how many versions are waiting.
- If the sidebar says the cloud needs reconnecting, it's not the network: the sign-in expired or the password changed. Reconnect in Settings → Cloud.

## Played offline on two PCs

Both have saves the other doesn't, so the next sync shows a [conflict](./conflicts.md). Pick one; the other stays in the version history.
