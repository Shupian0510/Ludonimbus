# Download and install

**Requirements**: Windows 10 / 11 (64-bit) with Steam installed. Only Windows is supported for now.

## Download and install

1. Open [GitHub Releases](https://github.com/Shupian0510/Ludonimbus/releases/latest) and download `Ludonimbus_x.y.z_x64-setup.exe` (x.y.z is the version).
2. Run the installer. It installs for the current user and **does not need administrator rights**.
3. Open Ludonimbus and follow the [Quick start](./quick-start.md).

Besides Ludonimbus itself, the installer contains:

- `ludonimbus-run.exe`: the small program Steam shortcuts start; it pulls before launch and uploads after exit.
- The official [Ludusavi](https://github.com/mtkennerly/ludusavi) program, used to find the saves of thousands of PC games.
- The Steam plugin file. It is only installed into Steam when you click Install in Settings; see [Cloud status inside Steam](../steam/status-in-steam.md).

## If Windows or your browser blocks it

The installer is not code-signed yet, so it may be stopped on the way:

- **The browser says the file "isn't commonly downloaded"**: in Edge, hover over the download, click "…" → "Keep" → "Show more" → "Keep anyway"; in Chrome, click "Keep".
- **Running it shows "Windows protected your PC" or "Unknown publisher"**: click "More info" → "Run anyway".

<!-- screenshot: smartscreen-warning (real-environment screenshot needed) -->

::: tip Why it isn't signed
Code-signing certificates cost money every year. Ludonimbus is open source; the installer is built automatically by GitHub Actions from the [public source code](https://github.com/Shupian0510/Ludonimbus), and you can build it yourself too.
:::

::: info Interface language
Ludonimbus 0.4 shows its interface in Chinese. This guide names buttons and menus in English; the [Quick start](./quick-start.md#labels) has a short list of the Chinese names.
:::

## Updates

When a new version is out, the Ludonimbus sidebar shows "New version x.y.z available". Open it to read what changed:

- "Update and restart" downloads and installs it, then reopens Ludonimbus. The installer's signature is verified, and your saves and settings are not affected.
- "Later" reminds you next time.
- You can also click "Check for updates" in Settings → About.

See the [Changelog](../reference/changelog.md) for what changed in each version.

## Uninstall {#uninstall}

Before uninstalling, put Steam back the way it was:

1. On each game's page, turn off "Steam auto-sync"; its Steam shortcut goes back to how it was. For games Ludonimbus added to Steam, click "Remove from Steam".
2. If you installed the Steam plugin, click "Remove plugin" in Settings → "Show cloud save status in Steam".
3. Uninstall Ludonimbus in Windows Settings → Apps.

Your saves and versions stay in the cloud; reinstall Ludonimbus and connect the same cloud to get them back. Local settings and sync records are in `%APPDATA%\Ludonimbus`; delete that folder if you no longer need it.
