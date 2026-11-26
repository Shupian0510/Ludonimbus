# Switch console (preview)

A Switch running Atmosphère can sync its saves through Ludonimbus on your PC. The saves on the console and the same game in Ryujinx or Eden are one progress: play on the console, continue in the emulator, and the other way round.

It is a **preview**: the client on the Switch, [Ludonimbus-NX](https://github.com/Shupian0510/Ludonimbus-NX), is still in development. See its repository for how to install it and where it stands.

## How it works

- The Switch talks to your PC over the **local network** only. The cloud, the history of saves and conflicts are handled by Ludonimbus on the PC, so Ludonimbus has to be running on the PC while the Switch syncs.
- In Ludonimbus the Switch is one more device, named "Switch"; its saves are on the same card as that game in the emulator.
- The client reads and writes the saves the system holds; it doesn't need `prod.keys`.

## Turning it on, on the PC

1. In Settings → "Switch", turn on "Allow Switch connections". It is off by default.
2. The first time Windows asks whether Ludonimbus may access the network, choose Private networks.
3. Click "Pair a new Switch". The PC shows a pairing code: enter it on the Switch. The code is good for 5 minutes and stops working after 5 wrong tries.
4. While pairing, compare the certificate fingerprint the PC shows with the one on the Switch. After that the Switch only accepts this PC.

Settings → "Switch" lists the paired consoles and when each last connected; any of them can be removed.

## With the other versions

- **Switch emulators**: the same game shares progress between the console, Ryujinx and Eden automatically.
- **The PC version**: its save format is usually different, so the two are kept apart; for games in the rule list, "Send to…" moves saves across, see [Several versions of one game](../sync/game-versions.md#send).
- A save made by a newer version of the game in an emulator may not be readable by an older version on the console: update the game on the console to the same version first.
