# 下载与安装

**系统要求**：Windows 10 / 11（64 位），已安装 Steam。目前只支持 Windows。

## 下载并安装

1. 打开 [GitHub Releases](https://github.com/Shupian0510/Ludonimbus/releases/latest)，下载 `Ludonimbus_x.y.z_x64-setup.exe`（x.y.z 是版本号）。
2. 运行安装程序。装在当前用户下，**不需要管理员权限**。
3. 安装完成后打开 Ludonimbus，按[快速上手](./quick-start.md)配置。

安装包里除了 Ludonimbus 本身，还有：

- `ludonimbus-run.exe`：Steam 快捷方式实际启动的小程序，负责启动前拉取、退出后上传。
- [Ludusavi](https://github.com/mtkennerly/ludusavi) 的官方程序：用来查找上万款 PC 游戏的存档位置。
- Steam 插件文件：只有你在设置里点「一键安装」时才会装进 Steam，见[在 Steam 里显示云存档状态](../steam/status-in-steam.md)。

第一次打开时会弹出[首次使用引导](./first-run.md)，带你连接云盘、开始添加游戏。

## 安装时被 Windows 或浏览器拦下

安装包暂未进行代码签名，下载和安装时可能会被拦一下：

- **浏览器提示「通常不会下载此文件」**：Edge 里把鼠标移到这条下载上，点「…」→「保留」→「显示更多」→「仍然保留」；Chrome 里点「保留」。
- **运行时提示「Windows 已保护你的电脑」或「未知发布者」**：点「更多信息」→「仍要运行」。

<!-- screenshot: smartscreen-warning (需要真实环境截图) -->

::: tip 为什么没有签名
代码签名证书需要每年付费购买。Ludonimbus 已经申请 SignPath Foundation 为开源项目提供的免费代码签名，通过后发布的安装包会带签名（见下面的[代码签名政策](#code-signing-policy)）。在那之前，安装包由 GitHub Actions 从[公开的源码](https://github.com/Shupian0510/Ludonimbus)自动构建，你也可以自己从源码构建。
:::

## 代码签名政策 {#code-signing-policy}

免费代码签名由 [SignPath.io](https://about.signpath.io/) 提供，证书由 [SignPath Foundation](https://signpath.org/) 颁发（Free code signing provided by SignPath.io, certificate by SignPath Foundation）。

- 只有 GitHub Actions 从[公开源码](https://github.com/Shupian0510/Ludonimbus)自动构建的安装包才会签名，每个版本签名前都由维护者手动批准。签名后的文件里，发布者显示为「SignPath Foundation」。
- 提交者与审核者：[Shupian0510](https://github.com/Shupian0510)
- 签名批准者：[Shupian0510](https://github.com/Shupian0510)
- 隐私：Ludonimbus 不向开发者发送任何数据，只连接你自己设置的服务（你的云盘、用你自己 Key 的 SteamGridDB），以及 GitHub：检查更新、下载 Ludusavi 的游戏数据库。详见[隐私政策](../privacy.md)。
- 目前正在向 SignPath Foundation 申请，通过之前的安装包没有签名。

## 更新

有新版本时，Ludonimbus 左侧栏会出现「新版本 x.y.z 可用」，点开能看到更新内容：

- 点「更新并重启」：自动下载并安装，完成后重新打开。安装包经过签名校验，存档和设置不受影响。
- 点「以后再说」：下次再提醒。
- 也可以在「设置 → 关于」里点「检查更新」。

每个版本改了什么见[更新日志](../reference/changelog.md)。

## 卸载 {#uninstall}

卸载前建议先做这几件事，让 Steam 恢复原样：

1. 在每个游戏的详情页关闭「Steam 自动同步」，Steam 快捷方式会恢复成原来的样子。Ludonimbus 帮你加进 Steam 的游戏，可以点「从 Steam 移除快捷方式」。
2. 装过 Steam 插件的话，在「设置 → 在 Steam 中显示云存档状态」里点「移除插件」。
3. 在 Windows「设置 → 应用」里卸载 Ludonimbus。

云端的存档和版本都会保留，以后重新安装 Ludonimbus 并连接同一个云盘就能找回。本机的设置和同步记录在 `%APPDATA%\Ludonimbus`，不再需要时可以手动删除。
