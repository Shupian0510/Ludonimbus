# Choosing a cloud

Ludonimbus doesn't provide storage; your saves live in your own cloud. You connect one cloud at a time and can switch whenever you like, copying the whole history over (see [Switching clouds](./switching.md)).

Saves are usually small (a few KB to tens of MB) and old automatic versions are cleaned up, so they take little space.

## Comparison

| Cloud | How you connect | Good for | Notes |
|---|---|---|---|
| [Google Drive](./google-drive.md) | Sign in in the browser | Most people outside mainland China | 15 GB free; Ludonimbus only sees the files it created |
| [Quark](./quark.md) | Scan a QR code with the Quark app | Quark users (mostly in China) | Experimental; connects through an OpenList that Ludonimbus runs; uploads are slower than other clouds (about 10 s per version) |
| [NAS (Synology / QNAP)](./webdav.md#synology) | WebDAV (or MinIO) | You have a NAS at home | Using it away from home needs remote access to the NAS |
| [Nextcloud](./webdav.md#nextcloud) | WebDAV | Self-hosted cloud | Use an app password if two-factor sign-in is on |
| [Jianguoyun](./webdav.md#jianguoyun) | WebDAV, account + app password | Users in mainland China | The free plan has monthly upload / download limits |
| [OpenList / AList](./webdav.md#openlist) | WebDAV | Baidu Netdisk, Aliyun Drive and other netdisks | You run OpenList yourself; speed and reliability depend on the netdisk driver |
| [Baidu Netdisk](./baidu.md) | Sign-in | — | One-click sign-in is waiting for Baidu's review and not available yet; use OpenList for now |
| [S3-compatible storage](./s3.md) | Access key | Cloudflare R2, Aliyun OSS, Tencent COS, MinIO on a NAS | Pay as you go (saves are small, so it's cheap); create the bucket first |
| [Local / synced folder](./folder.md) | Pick a folder | OneDrive / Dropbox / Google Drive desktop sync folders, NAS shares, USB drives | For several PCs, the folder itself must be synced or shared between them |

## How to pick

- **Easiest**: [Google Drive](./google-drive.md), sign in once in the browser.
- **You already use the OneDrive or Dropbox desktop app**: pick its sync folder, see [Local / synced folder](./folder.md).
- **You have a NAS**: Synology and QNAP both have [WebDAV](./webdav.md) built in, and your saves stay at home.
- **You already have object storage**: use [S3-compatible storage](./s3.md).
- **You use Quark**: [Quark](./quark.md), sign in with a QR code (experimental).
- **You want Baidu Netdisk or Aliyun Drive**: mount the netdisk in [OpenList / AList](./webdav.md#openlist), then connect over WebDAV. One-click Baidu sign-in will come in a new version once Baidu approves it.

::: tip Several PCs
All your PCs must connect to the **same** cloud (the same account, or the same server and folder) to see the same saves; see [Multiple devices](../sync/devices.md). If you only use one PC and don't want to sign up for anything, a USB drive or NAS folder works too.
:::
