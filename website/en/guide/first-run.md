# First-run guide

The first time you open Ludonimbus (no cloud connected, no games added), a short guide walks you through the basic setup. Every step can be skipped, and the close button in the top right ends it; you can open it again from Settings → About → "Show the first-run guide again".

## 1. Welcome

What Ludonimbus does. Below it you can switch the interface language (简体中文 / English); you can also change it later in [Settings](../reference/settings.md#language).

![First-run guide: welcome](/screenshots/en/guide-welcome.png)

## 2. What you want to use it for

Pick any that apply: emulator games, recomps and ports, cloud saves for third-party games in Steam, syncing saves across devices. Ludonimbus adjusts the suggestions in the next steps to match. Your answers stay on this PC.

![First-run guide: what you use it for](/screenshots/en/guide-usage.png)

## 3. Connect a cloud

Clouds that suit you come first: in English, Google Drive, WebDAV (NAS, Nextcloud) and a synced folder; the rest are under "More clouds…". Connecting works exactly as in Settings; see [Choosing a cloud](../clouds/index.md). Click Next once it's connected.

If you picked "Sync saves across devices", connect every device to the **same** cloud; see [Multiple devices](../sync/devices.md).

![First-run guide: connect a cloud](/screenshots/en/guide-cloud.png)

## 4. Artwork (optional)

With a free SteamGridDB API key, Ludonimbus can pick the cover, background, logo and icon when you add games to Steam. Follow the three steps on the page to get a key; details in [Artwork and SteamGridDB](../steam/artwork.md). You can skip this and enter the key in Settings later, or use local images.

![First-run guide: artwork](/screenshots/en/guide-art.png)

## 5. Show cloud save status in Steam (optional)

"Download and install" installs Millennium, an open-source plugin framework for the Steam client, together with the Ludonimbus plugin. Game pages in your Steam library then show the cloud save status, with a sync button. Steam closes during the install and reopens afterwards. Details in [Cloud save status in Steam](../steam/status-in-steam.md).

Auto-sync works without it, so you can skip this and install it in Settings later. If Steam isn't found, this step only tells you to skip it.

![First-run guide: cloud save status in Steam](/screenshots/en/guide-steam.png)

## 6. Done

Shows what's set up and what was skipped, with suggestions for what to do next based on your answers. "Scan this PC" opens [Scan this PC](../games/scan.md) right away so you can add games.

If you find Ludonimbus useful, the "Star on GitHub" button on this page is a great way to support it.

![First-run guide: done](/screenshots/en/guide-done.png)

## What next

Follow steps 2–4 of the [Quick start](./quick-start.md) to add games and turn on Steam auto-sync.
