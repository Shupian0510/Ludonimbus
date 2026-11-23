# Cloud status inside Steam

Once installed, the play bar of every Ludonimbus-managed game in your Steam library gets a "Ludonimbus cloud saves" item, next to "Last played" and "Play time".

<!-- screenshot: steam-play-bar-status (real-environment screenshot needed) -->

- It updates live while launching, playing and uploading, for example "up to date · 5 minutes ago", "game running, uploads after exit", "uploading saves…" or "conflict, click to choose which to keep".
- Click the button on the right to sync now; click the status itself to open the game in Ludonimbus.
- Hover for details: when and from which device it last synced, the newest version in the cloud, and how many versions are waiting to upload.
- It only appears for games Ludonimbus manages; regular Steam games are not affected.

## Install

1. Open Settings → "Show cloud save status in Steam" and click "Install" ("Enable plugin" if Millennium is already installed).
2. The confirmation explains what will happen; click "Download and install".
3. Ludonimbus then:
   - downloads the latest stable [Millennium](https://github.com/SteamClientHomebrew/Millennium) from its official GitHub releases, **verifies its SHA-256**, and installs it into the Steam folder;
   - puts the Ludonimbus plugin into Millennium and enables it;
   - closes Steam during the install and reopens it afterwards.

<!-- screenshot: settings-steam-plugin -->

This is the same thing Millennium's official installer does. Millennium updates itself from then on; the plugin is updated together with Ludonimbus and takes effect the next time Steam starts.

::: info What is Millennium
Millennium is an open-source plugin framework for the Steam client. It is **not an official Valve component**. Ludonimbus doesn't bundle it; it's downloaded from the official releases only when you click Install.
:::

## Good to know

- **Ludonimbus must be running in the tray.** Otherwise the item says "Ludonimbus is not running"; Steam and your games are unaffected, and auto-sync still works.
- It only appears in Steam's desktop mode, not in Big Picture.
- Old Millennium versions (2.x) aren't supported; update with Millennium's official installer first.

## Remove

Click "Remove plugin" in the same place. Steam stops showing the cloud status; auto-sync is unaffected. Millennium itself stays; uninstall it with Millennium's official installer.

## Is it safe?

The plugin only displays information and forwards clicks. It asks the Ludonimbus tray app for the status through a local interface that only listens on this PC and requires a token generated at every start, so scripts on web pages can't call it. The plugin never touches your cloud account.
