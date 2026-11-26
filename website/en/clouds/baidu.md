# Baidu Netdisk

::: info Status
Ludonimbus has no one-click Baidu Netdisk sign-in. An app on the Baidu Netdisk open platform has to pass a review before everyone can use it, and that review is not being pursued, so Settings has no Baidu Netdisk entry.
:::

## Using Baidu Netdisk

Connect through [OpenList](https://github.com/OpenListTeam/OpenList) / AList:

1. Mount your Baidu Netdisk in OpenList.
2. In Ludonimbus, connect to OpenList over WebDAV, with the folder inside the mounted netdisk, for example `Baidu/Ludonimbus`.

Step by step: [WebDAV → OpenList / AList](./webdav.md#openlist). OpenList is an independent third-party open-source tool; speed and reliability depend on Baidu and on OpenList's driver.

## Speed

Baidu throttles downloads for non-members, so pulling large saves (or a [game backup](../features/game-backup.md)) takes a while.

## Other choices

[Quark](./quark.md) connects with a QR code, and [Jianguoyun](./webdav.md#jianguoyun) only needs an account and an app password. If you change clouds later, your saves can be copied over in one click; see [Switching clouds](./switching.md).
