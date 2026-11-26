# FAQ and troubleshooting

## FAQ

### Does it sync if I don't start the game from Steam?

It uploads, but it can't pull before launch. With Ludonimbus in the tray, it notices the game exiting and backs it up. To get the newest saves before you play, start games from Steam.

### I played on two PCs and now there's a conflict. What do I do?

Before the game starts, Ludonimbus asks whether to use the cloud's saves or this PC's. Both are kept in the version history, so you can restore the other one at any time. See [Conflicts](../sync/conflicts.md).

### Can I use Baidu Netdisk or Quark?

Quark connects in one click with a QR code (experimental); see [Quark](../clouds/quark.md). One-click Baidu Netdisk sign-in uses the official API of the Baidu Netdisk open platform and is waiting for Baidu's review; it will come in a new version once approved, see [Baidu Netdisk](../clouds/baidu.md). Ludonimbus provides no netdisk aggregation of its own; if you run a WebDAV service such as OpenList / AList yourself, you can connect it as described under [WebDAV](../clouds/webdav.md#openlist), with its configuration and compliance being your own responsibility.

### Can I switch the interface to English or Chinese?

Yes. Pick it in Settings → "Language / 语言"; it applies right away. By default it follows the Windows display language. See [Settings](./settings.md#language).

### Quark says the sign-in has expired. What do I do?

Click "Scan to sign in again" in Settings → Cloud and scan with the Quark app once more. The component isn't downloaded again and your saves aren't affected. See [Quark](../clouds/quark.md#when-the-sign-in-expires).

### Do Xbox app (PC Game Pass) games work?

They can be added to Steam. An Xbox game's own program is protected and can't be started by other programs, so Ludonimbus starts it through `gamelaunchhelper.exe` in the game folder and uploads saves after the game itself exits. When picking a program by hand, choose `gamelaunchhelper.exe` in the `Content` folder too. See [Add to Steam](../steam/add-to-steam.md#xbox).

### Why does the scan say Steam Cloud already syncs some games?

Those games are installed in Steam and Steam Cloud already syncs them, so Ludonimbus doesn't need to. If you uninstall one from Steam and play it on another store, it shows up under "Ready to add".

### What happens to my saves when I switch clouds?

After you connect the new cloud, Settings offers to copy the saves from the old one, with their full history. The old cloud is never changed. See [Switching clouds](../clouds/switching.md).

### Are my saves still there after deleting a game or reinstalling Windows?

Yes. They're in your cloud: reinstall Ludonimbus, connect the same cloud and add the same game, and they're restored on the first launch.

### Does Ludonimbus change my Steam?

Only the non-Steam shortcuts you turned auto-sync on for, or that it added to Steam for you. Turning auto-sync off restores the original shortcut. The Steam plugin is optional and only installed when you click Install.

### What should I do before uninstalling Ludonimbus?

Turn off "Steam auto-sync" on each game's page so its Steam shortcut goes back to how it was. Your saves and versions stay in the cloud. See [Download and install → Uninstall](../guide/install.md#uninstall).

### Does it support macOS or Linux?

Only Windows for now.

## Troubleshooting

### Where are the logs?

Logs are in `%APPDATA%\Ludonimbus\logs\` (Settings → About → Data folder opens the parent folder):

| File | What it records |
|---|---|
| `runner.log` | Syncing when a game is started from Steam |
| `sync.log` | The result of every sync |
| `watcher.log` | The tray watcher (games started outside Steam) |
| `agent.log` | The local interface used by the Steam plugin |
| `steam-plugin.log` | Installing and updating the Steam plugin |

When reporting a problem, the relevant log helps a lot (have a look first and remove any paths or names you'd rather not share).

### Common problems

| What you see | What to do |
|---|---|
| The sidebar says the cloud needs reconnecting | The sign-in expired or the password changed; reconnect in Settings → Cloud |
| "Test connection" reports a certificate error | The NAS has no trusted HTTPS certificate; use an `http://` address |
| Jianguoyun / Nextcloud say the user name or password is wrong | They need an app password, not your sign-in password; see [WebDAV](../clouds/webdav.md) |
| S3 says the bucket doesn't exist, or a signature error | Check the endpoint, region and bucket name; see [S3-compatible storage](../clouds/s3.md#troubleshooting) |
| Starting from Steam doesn't sync | Check that "Steam auto-sync" on the game page says it syncs when started from Steam; click "Repair" if the shortcut was changed |
| `ludonimbus-run.exe` can't be found | Reinstall Ludonimbus |
| No cloud status in Steam | Make sure Ludonimbus is running in the tray; the status only shows in desktop mode and for games Ludonimbus manages; after a plugin update, restart Steam |
| The Steam play bar says Ludonimbus isn't running | Open Ludonimbus (it stays in the tray when you close the window) |
| An old Millennium (2.x) is detected | Update it with Millennium's official installer first |
| "Play and find" found no changes | Make sure the game really saved and that you quit it; or choose the folder yourself |
| Large saves, may include caches | Click "View" to see what's inside, and exclude cache folders under the game's Settings → Exclude |
| Ryujinx asks to be closed on another PC | Ryujinx can't be open while Ludonimbus creates the save on this PC; close it and start again |
| Antivirus blocks it | The installer isn't code-signed, so some antivirus tools may flag it; add the Ludonimbus install folder to the trusted list |

### Still stuck? {#still-stuck}

Open an issue on [GitHub Issues](https://github.com/Shupian0510/Ludonimbus/issues) with your Ludonimbus version (Settings → About), the cloud you use, the game and the steps, plus the relevant logs.
