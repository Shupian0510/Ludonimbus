# Baidu Netdisk (in review)

::: info Status
One-click Baidu Netdisk sign-in is ready and **waiting for Baidu's open-platform review**; it will be enabled in a new version once approved. Until then, release builds don't show Baidu Netdisk in Settings.
:::

## Using Baidu Netdisk today

Connect through [OpenList](https://github.com/OpenListTeam/OpenList) / AList:

1. Mount your Baidu Netdisk in OpenList.
2. In Ludonimbus, connect to OpenList over WebDAV, with the folder inside the mounted netdisk, for example `Baidu/Ludonimbus`.

Step by step: [WebDAV → OpenList / AList](./webdav.md#openlist). Speed and reliability depend on Baidu and on OpenList's driver; Baidu throttles downloads for non-members, so pulling large saves takes a while.

## Once it is available

With one-click sign-in you just authorise your Baidu account, no OpenList needed. Saves will live in the 我的应用数据/Ludonimbus (My App Data/Ludonimbus) folder of your netdisk, and Ludonimbus can only access that folder. Saves you stored through OpenList can be copied over in one click; see [Switching clouds](./switching.md).
