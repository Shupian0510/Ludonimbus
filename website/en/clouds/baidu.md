# Baidu Netdisk (in review)

::: info Status
One-click Baidu Netdisk sign-in is ready and **waiting for Baidu's open-platform review**; it will be enabled in a new version once approved. Until then, release builds don't show Baidu Netdisk in Settings.
:::

## Connecting

1. Open Settings → Cloud and click "Connect Baidu Netdisk".
2. Ludonimbus opens Baidu's official authorization page in your browser and shows a code and a QR code.
3. Sign in to your Baidu account on Baidu's page (or scan the QR code with the Baidu Netdisk app), review the permissions and confirm.
4. Ludonimbus continues by itself; Settings then shows your netdisk nickname and storage use.

Your Baidu account and password are only entered on Baidu's page; Ludonimbus never sees them. The authorization is stored in the Windows Credential Manager on your PC.

## What Ludonimbus can access

- Ludonimbus uses the official API of the Baidu Netdisk open platform with the scope `basic` and `netdisk`.
- **It only reads and writes the folder 我的应用数据/Ludonimbus (My App Data/Ludonimbus)** in your netdisk, which holds your saves, version metadata and device information and is visible on the website and in the app. No other file in your netdisk is listed, read, changed or deleted.
- Baidu's open platform also restricts apps to their own folder on the server side.

Details: [Privacy Policy · Baidu Netdisk](../privacy.md#baidu-netdisk).

## Deleting and restoring

- Deleting a version in a game's version history moves it to Recently deleted below the list, where you can **restore it for 30 days**; after that it is deleted for good. You can also delete it for good from there.
- A restored automatic version is locked too, so automatic cleanup doesn't delete it again right away.
- To remove everything, delete the folder 我的应用数据/Ludonimbus in Baidu Netdisk.

## Revoking access {#revoke}

1. **In Ludonimbus**: Settings → Cloud → Disconnect. This deletes the Baidu authorization stored on this PC.
2. **On Baidu's side**: sign in to your Baidu account, open 账号设置 → 授权管理 (Account settings → Authorization management, [passport.baidu.com/v6/appAuthority](https://passport.baidu.com/v6/appAuthority)), find Ludonimbus and click 取消授权 (Revoke). The Ludonimbus authorization stops working on every PC.

"How do I revoke access?" under Baidu Netdisk in Settings shows the same steps and opens Baidu's page directly. Revoking access doesn't delete the saves in your netdisk.

## Using your own Baidu app (advanced)

Click "Use my own Baidu app (advanced)" in Settings to enter the AppKey, SecretKey and app name of an app you created on the [Baidu Netdisk open platform](https://pan.baidu.com/union); saves then go in `我的应用数据/<your app name>`. The SecretKey is kept only in the Windows Credential Manager on your PC, never in the settings file, and is only sent to Baidu when signing in and renewing the authorization.

## Speed

Baidu throttles downloads for non-members, so pulling large saves (or [game backups](../features/game-backup.md)) takes a while.

## About OpenList / AList

Ludonimbus never connects to Baidu Netdisk through OpenList, AList or any other third-party tool; Baidu Netdisk is accessed only through the official open-platform API. Ludonimbus provides no netdisk aggregation or mounting of its own; which server a [WebDAV](./webdav.md) connection goes to is entirely your own configuration.
