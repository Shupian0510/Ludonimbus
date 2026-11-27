# Game rules / 游戏规则

`rules/games/<id>.yaml` is one game whose saves are the same files on several platforms: its versions there share one save line (on the first time, Ludonimbus asks once). Each file is built into the program, and comes with a small sample of the saves on each platform.

`rules/games/<id>.yaml` 是一个游戏的规则：它在几个平台上存的是同样的文件，这几个平台的版本可以共用一个进度（第一次会问一次）。每个文件编译进程序，旁边带一份各平台的样例存档。

Programs (emulators, recomps, launchers) are not described here: they are in `adapters/`.

模拟器、重编译游戏、启动器的规则不在这里，在 `adapters/`。

## Adding a game / 加一个游戏

1. Write `rules/games/<id>.yaml` (see below and `stardew-valley.yaml`).
2. Put a save folder for at least two platforms next to it, as the game leaves it there: `rules/games/<id>/<platform>/…`. A few small files are enough; it doesn't have to be a real playthrough, but its layout and file names must be the game's.
3. Run `ludonimbus rules check rules/games/<id>.yaml` (or `cargo run -p ludonimbus-cli -- rules check …`). CI runs the same check on every file.
4. Say in the pull request how you checked where the saves are and that the files really are the same on both platforms.

1. 写 `rules/games/<id>.yaml`（字段见下面，参考 `stardew-valley.yaml`）。
2. 旁边放至少两个平台的存档文件夹，照游戏在那个平台上留下的样子：`rules/games/<id>/<平台>/…`。几个小文件就够，不用是真的进度，但文件夹结构和文件名要和游戏的一样。
3. 运行 `ludonimbus rules check rules/games/<id>.yaml`（或者 `cargo run -p ludonimbus-cli -- rules check …`）。CI 会对每个文件做同样的检查。
4. 在 PR 里写清楚你是怎么核对存档位置、怎么确认两个平台上的文件确实一样的。

## Fields / 字段

```yaml
id: stardew-valley              # the file's name; lowercase letters and digits joined by "-"
name: Stardew Valley
names: { zh-CN: 星露谷物语 }     # optional: the name in other languages
ids:                            # which versions it covers
  - nx:0100E65002BB8000         #   a Switch game (console or emulator), by title id
  - ludusavi:stardew valley     #   a PC version Ludusavi knows, by its title (lowercase)
  - name:stardewvalley          #   any version with this name (letters and digits, lowercase)
lines:
  - id: main                    # one line per game for now
    files: "*/*"                # which files in the save folder are saves (default: all);
                                #   any letter case, * doesn't cross /
    platforms:                  # where the save folder is on each platform
      pc: { saves: StardewValley/Saves }   # the ending of the folder's path
      switch: { saves: "{nxsave}" }        # a Switch game's save data itself
    version:                    # optional: how to read the game version that wrote a save
      file: "*/SaveGameInfo"    #   a file among the saves
      xml: gameVersion          #   the XML element with the version
      newerOnly: true           #   an older game can't load a save a newer one wrote
    checked:                    # what has been tried for real, by pair of platforms
      pc-switch: untested. …
```

- Platforms are `pc`, `switch`, or the key of a program in `adapters/` (an emulator).
- With `newerOnly`, a save made on another platform by a newer game is not brought over by itself; the user is told first.
- Files in the save folder that `files` doesn't name (settings and the like) stay on each platform.

- 平台是 `pc`、`switch`，或者 `adapters/` 里某个程序（模拟器）的 key。
- 写了 `newerOnly`，另一个平台上用更新版本的游戏存的档不会自动换过来，会先告诉用户。
- 存档文件夹里 `files` 没写到的文件（设置之类）留在各自的平台上。
