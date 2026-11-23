# Google Drive

Use your own Google account; saves are kept in your Google Drive.

## Connect

1. Open Settings → Cloud and click "Connect Google Drive".
2. Your browser opens Google's sign-in page. Choose your account and allow access.
3. Back in Ludonimbus, the cloud section shows "Connected", your account and the space used.

<!-- screenshot: settings-cloud-google -->

## What Ludonimbus can see

- **Only the files Ludonimbus itself created** (Google's `drive.file` permission). It cannot see or change any other file in your Drive.
- Saves are in a "Ludonimbus" folder in your Drive; you can browse and download them on the Drive website. Each version is an ordinary zip.
- The sign-in token is stored in the Windows Credential Manager on your PC.

See the [Privacy policy](../privacy.md).

## Reconnect or disconnect

- Sign-in expired, or another account: click "Reconnect Google Drive".
- No longer needed: click "Disconnect" on the right of the cloud section. This only stops syncing on this PC and deletes the sign-in token from it; your saves and versions stay in the cloud.
- To revoke access completely, remove Ludonimbus at [myaccount.google.com/permissions](https://myaccount.google.com/permissions).
- To delete all cloud data, delete the "Ludonimbus" folder in Google Drive.

## Use your own client ID (advanced)

Release builds include the OAuth client Google sign-in needs, so most people can ignore this. To use a "Desktop app" client you created in Google Cloud, click "Use your own client ID (advanced)", enter the client ID and client secret, then connect.
