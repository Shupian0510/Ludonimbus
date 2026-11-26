# WebDAV: NAS, Nextcloud, OpenList

WebDAV is a standard protocol supported by many clouds and NAS systems. Synology, QNAP, Nextcloud and Jianguoyun have it built in; Baidu Netdisk, Aliyun Drive and others can be mounted in OpenList / AList and reached through its WebDAV server.

## Steps

1. Open Settings → Cloud and click "Enter WebDAV details…".
2. Under "Presets", click the service you use (Jianguoyun, Synology NAS, OpenList / AList, Nextcloud). Ludonimbus fills in the address format and shows notes for that service.
3. Fill in:
   - **Address**: the WebDAV address. Replace the parts in angle brackets `<…>` with your own, for example `<NAS address>` with `192.168.1.10`.
   - **User name** and **Password**.
   - **Folder**: where saves go, `Ludonimbus` by default. It is created if it doesn't exist.
4. Click "Test connection" to check that Ludonimbus can connect and write.
5. Click "Connect".

![Settings → Cloud: WebDAV (Jianguoyun)](/screenshots/en/settings-cloud-webdav.png)

The password is kept in the Windows Credential Manager, never in the config file, and removed when you disconnect. To change the address or password later, edit it in the same place and click "Save changes"; leave the password empty to keep the saved one.

::: tip NAS on your local network and HTTPS
If your NAS has no trusted HTTPS certificate (for example a self-signed one), "Test connection" reports a certificate error. Use an `http://` address instead.
:::

## Jianguoyun {#jianguoyun}

| Field | Value |
|---|---|
| Address | `https://dav.jianguoyun.com/dav/` |
| User name | Your Jianguoyun account e-mail |
| Password | An **app password**, not your sign-in password |
| Folder | `Ludonimbus` (created at the top of your Jianguoyun) |

To create an app password, sign in on the Jianguoyun website, open Account info → Security → Third-party apps, add an app (any name, such as Ludonimbus) and copy the generated password.

The free plan has monthly upload and download limits, which are usually enough for saves.

## Synology {#synology}

1. In DSM, install and enable "WebDAV Server" from the Package Center, and turn on HTTPS (port 5006 by default) or HTTP (port 5005 by default).
2. In Ludonimbus, click "Synology NAS":

| Field | Value |
|---|---|
| Address | `https://<NAS address>:5006/`; if the certificate isn't trusted, `http://<NAS address>:5005/` |
| User name / password | Your DSM account |
| Folder | `<shared folder>/Ludonimbus`, for example `home/Ludonimbus` |

The folder must be inside a shared folder you can write to.

## QNAP {#qnap}

1. In QTS, open Control Panel → Network & File Services → Win/Mac/NFS/WebDAV → WebDAV and select "Enable WebDAV". "Shared folder permission" is fine.
2. Note the HTTP / HTTPS ports shown on that page (WebDAV may use the web server's ports or its own).
3. In Ludonimbus, enter:

| Field | Value |
|---|---|
| Address | `http://<NAS address>:<port>/` |
| User name / password | Your QTS account |
| Folder | `<shared folder>/Ludonimbus`, for example `Public/Ludonimbus` |

## Nextcloud {#nextcloud}

| Field | Value |
|---|---|
| Address | `https://<server>/remote.php/dav/files/<user name>/` |
| User name | Your Nextcloud user name |
| Password | Your password; with two-factor sign-in, an app password |
| Folder | `Ludonimbus` |

Create app passwords in Nextcloud under Personal settings → Security → Devices & sessions.

## OpenList / AList {#openlist}

::: tip Quark
Since 0.5, Quark [connects in one click](./quark.md): Ludonimbus downloads and runs OpenList itself, with nothing to set up. The steps below are for when you already run OpenList or want another netdisk.
:::

[OpenList](https://github.com/OpenListTeam/OpenList) (a community fork of AList) can mount Baidu Netdisk, Aliyun Drive, Quark and many other netdisks and serve them over WebDAV. Ludonimbus has no direct connection to Baidu Netdisk or Aliyun Drive, so OpenList is how to use them today. OpenList is an independent third-party open-source tool; whether using it fits a netdisk's user agreement is for you to judge.

1. Install and run OpenList (the Windows program or Docker), open its admin page and add your netdisk as a storage, following OpenList's documentation. Note the mount path, for example `/Quark`.
2. The OpenList user you connect with needs WebDAV read and manage permissions (the admin account has them).
3. In Ludonimbus, click "OpenList / AList":

| Field | Value |
|---|---|
| Address | `http://<address>:5244/dav/`; use `127.0.0.1` when OpenList runs on this PC |
| User name / password | Your OpenList account |
| Folder | `<mounted netdisk>/Ludonimbus`, for example `Quark/Ludonimbus` |

Keep in mind:

- Put the folder inside the mounted netdisk; otherwise saves only end up on the OpenList machine.
- **OpenList must be running when Ludonimbus syncs.** If it runs on this PC, start it with Windows. When it isn't running, Ludonimbus treats the cloud as unreachable: saves stay on the PC and are uploaded later (see [Playing offline](../sync/offline.md)).
- Speed and reliability depend on the netdisk and OpenList's driver; some netdisks throttle downloads for non-members.
- With OpenList and Quark, saves go through the OpenList you run yourself to your Quark netdisk, with no third-party server in between.

## Can't connect? {#troubleshooting}

| What you see | Likely cause |
|---|---|
| 401 / wrong user name or password | Jianguoyun and Nextcloud (with two-factor sign-in) need an app password |
| Certificate error | The NAS has no trusted certificate; use `http://` |
| 404 / not found | The address is missing its path; Jianguoyun and OpenList addresses end in `/dav/` |
| 403 / forbidden | The folder isn't inside a shared folder you can write to; the OpenList user lacks WebDAV manage permission |
| Timeout | You're not on the same network, or the NAS isn't reachable from outside |

More in [FAQ and troubleshooting](../reference/faq.md).
