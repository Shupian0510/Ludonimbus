# Switching clouds

You can switch to another cloud at any time. Saves in the old cloud are never deleted, and you can copy the whole history over in one click.

## Switch

1. Connect the new cloud in Settings → Cloud.
2. Ludonimbus asks whether to switch; click "Switch". From now on it syncs to the new cloud.
3. Above the cloud section, Ludonimbus asks whether to copy the saves from your previous cloud:
   - "Copy saves" copies every version of every game from the old cloud to the new one, with progress, and tells you how many versions were copied.
   - "No thanks" leaves the old saves where they are; Ludonimbus only uses the new cloud from now on.

<!-- screenshot: settings-cloud-migrate -->

While copying:

- **The old cloud is never changed**, so it stays a complete backup.
- An interrupted copy is fine: click "Copy saves" again and it picks up where it stopped, without copying anything twice.
- Versions that are already in the new cloud are skipped.

## Disconnect

Click "Disconnect" on the right of the cloud section. This only stops syncing on this PC; **your saves and versions stay in the cloud**. WebDAV / S3 passwords and the Google sign-in token are removed from the PC.

Until you connect again, saves are only kept on this PC and the library shows that no cloud is connected. Reconnect the same cloud later and all versions are still there.

## Switching several PCs

Switch every PC to the new cloud in Settings. You only need to copy once, from one of them; the others see the same versions as soon as they connect. If they offer to copy too, click "No thanks" (copying again does no harm either; existing versions are skipped).
