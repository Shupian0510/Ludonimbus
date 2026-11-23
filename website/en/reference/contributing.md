# Contributing

Ludonimbus is open source (MIT). [Issues](https://github.com/Shupian0510/Ludonimbus/issues) and pull requests are welcome.

## Adding save rules

The most useful contributions are new emulator and game save rules. Rules are YAML files compiled into the app:

| File | Contents |
|---|---|
| [`adapters/builtin.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/builtin.yaml) | Recomps and ports |
| [`adapters/emulators.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/emulators.yaml) | Emulators |
| `adapters/recomplauncher.yaml` | Generated from RecompLauncher's catalog; **don't edit it by hand**. To fix a game, write a rule with the same key in `builtin.yaml` |

A game rule looks roughly like this:

```yaml
- key: soh                      # game id; your PCs find the same saves through it
  name: 塞尔达传说：时之笛
  platform: Ship of Harkinian（N64 移植）
  match: { exe: soh.exe }       # how to recognise it from its program
  saves: ["{install}/Save/**"]  # the saves to sync
```

Rules can also have `exclude` (files to leave out), `waitFor` (the game's real process name), `relocate` (how to find it after it moves) and more; each field is explained in the comments at the top of the file. Emulator rules also have `gameId` and `gameSaves` for per-game sync. (The `name` and `platform` fields are in Chinese for now.)

Before submitting:

- check the save location against the game's source code, official notes or [PCGamingWiki](https://www.pcgamingwiki.com/); list only saves, never the whole game folder, caches or logs;
- say in the pull request how you checked it;
- if you can, try adding the game with a development build.

How rules are matched and tested is described in the development notes under [built-in save rules](https://github.com/Shupian0510/Ludonimbus/blob/main/docs/development.md#内置存档规则).

Save locations of regular PC games come from Ludusavi's database, which is built from PCGamingWiki. If a PC game's location is wrong, fixing it on PCGamingWiki helps every tool that uses Ludusavi.

## Improving the docs and translations

This site's source is the [`website/`](https://github.com/Shupian0510/Ludonimbus/tree/main/website) folder of the repository, in plain Markdown: Chinese at the top level and English under `en/`, with matching file names. Every page has an "Improve this page on GitHub" link at the bottom.

- If you change one language, please update the other too; if you can't, say so in the pull request and someone will fill it in.
- Write button and menu names in quotes, the same way the app shows them.
- Mark where a screenshot should go with a comment; screenshots are generated together.

The repository's README also comes in Chinese (`README.md`) and English (`README.en.md`). If you'd like to help translate the app itself, open an issue first.

## Reporting problems

See [FAQ and troubleshooting](./faq.md#still-stuck).

## Building from source

You need Rust, Node.js and the Visual Studio C++ build tools; see [`docs/development.md`](https://github.com/Shupian0510/Ludonimbus/blob/main/docs/development.md).
