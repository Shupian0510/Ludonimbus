# 参与贡献

Ludonimbus 是开源项目（MIT 许可），欢迎提交 [Issue](https://github.com/Shupian0510/Ludonimbus/issues) 和 Pull Request。

## 补充存档规则

最有用的贡献是新的模拟器 / 游戏存档规则。规则是 YAML 文件，编译进程序：

| 文件 | 内容 |
|---|---|
| [`adapters/builtin.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/builtin.yaml) | 重编译 / 移植游戏 |
| [`adapters/emulators.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/emulators.yaml) | 模拟器 |
| `adapters/recomplauncher.yaml` | 从 RecompLauncher 目录自动生成，**不要手改**；要修正某个游戏，在 `builtin.yaml` 写同一个 key 的规则 |

一条游戏规则大概是这样：

```yaml
- key: soh                      # 游戏 id，多台电脑靠它找到同一份存档
  name: 塞尔达传说：时之笛
  platform: Ship of Harkinian（N64 移植）
  match: { exe: soh.exe }       # 怎么从游戏程序认出它
  saves: ["{install}/Save/**"]  # 要同步的存档
```

还可以写 `exclude`（排除的文件）、`waitFor`（游戏真正的进程名）、`relocate`（换了文件夹时怎么找到它）等，每个字段的说明在文件开头的注释里。模拟器规则还有按游戏同步用的 `gameId` 和 `gameSaves`。

提交前请：

- 按游戏源码、官方说明或 [PCGamingWiki](https://www.pcgamingwiki.com/) 核对存档位置，只写存档，不要写整个游戏目录、缓存或日志；
- 在 PR 里写上你是怎么核对的；
- 有条件的话，用开发版实际添加这个游戏试一次。

规则怎么匹配、怎么测试，见开发说明里的[内置存档规则](https://github.com/Shupian0510/Ludonimbus/blob/main/docs/development.md#内置存档规则)。

普通 PC 游戏的存档位置来自 Ludusavi 数据库，数据源是 PCGamingWiki。某个 PC 游戏的位置不对，最好直接去 PCGamingWiki 修正，所有用 Ludusavi 的工具都会受益。

## 改进文档和翻译

这个文档站的源文件在仓库的 [`website/`](https://github.com/Shupian0510/Ludonimbus/tree/main/website) 文件夹，是普通的 Markdown：中文在根目录，英文在 `en/` 下，两边的文件名一一对应。每页底部都有「在 GitHub 上改进这一页」的链接。

- 改了一种语言，请尽量同时更新另一种；不会写的话在 PR 里说明，会有人补上。
- 界面上的按钮和菜单名，中文用「」括起来，并和应用里的文字保持一致。
- 截图的位置先用注释标出来，截图会统一生成。

仓库首页的 README 也有中文（`README.md`）和英文（`README.en.md`）两份。想帮忙翻译应用界面，请先开一个 Issue 联系。

## 报告问题

见[常见问题与故障排查](./faq.md#still-stuck)。

## 从源码构建

需要 Rust、Node.js 和 Visual Studio C++ 生成工具，步骤见 [`docs/development.md`](https://github.com/Shupian0510/Ludonimbus/blob/main/docs/development.md)。
