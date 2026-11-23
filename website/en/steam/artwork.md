# Artwork and SteamGridDB

The Ludonimbus library uses the covers, banners and logos you set in Steam. For games that aren't in Steam, or that Ludonimbus adds to Steam, you can pick artwork from the [SteamGridDB](https://www.steamgriddb.com/) community or use local images.

## Enter a SteamGridDB API key

SteamGridDB needs a free API key:

1. Open [SteamGridDB's API page](https://www.steamgriddb.com/profile/preferences/api) and sign in with your Steam account.
2. Under Preferences → API, generate a key and copy it.
3. Paste it in Ludonimbus under Settings → "Artwork (SteamGridDB)" and click "Save".

<!-- screenshot: settings-sgdb -->
<!-- screenshot: sgdb-api-page (real-environment screenshot needed) -->

The key is kept in the Windows Credential Manager; click "Delete key" to remove it. Without a key you can still use local images.

## Automatic artwork

With "automatic artwork" on in [Add to Steam](./add-to-steam.md), Ludonimbus picks the top-rated images from SteamGridDB. Images you already chose are not replaced.

## Pick it yourself

On the game page, click "Change artwork":

1. Search for the game at the top (try the English name if nothing comes up) and select the right one.
2. Community images are listed under each image type; click one to use it.
3. To use your own image, click "Use a local image…".

<!-- screenshot: art-picker -->

| Image | Where it's shown |
|---|---|
| Cover | The cover in the library grid |
| Wide cover | Wide spots such as Recent Games |
| Background | The banner at the top of the game page |
| Logo | The title drawn over the background |
| Icon | The small icon in lists and the taskbar |

Steam shows the new artwork after it restarts.

<!-- screenshot: steam-library-artwork (real-environment screenshot needed) -->

## Privacy

Only when you search for artwork does Ludonimbus send the game name to SteamGridDB with your key, and it downloads the images you choose. No save data is sent. See the [Privacy policy](../privacy.md).
