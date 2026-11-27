# Switch 主机（预览）

装了 Atmosphère 的 Switch 可以通过电脑上的 Ludonimbus 同步存档：Switch 上的存档和 Ryujinx、Eden 里的同一个游戏是同一个进度，在主机上玩完，到模拟器里接着玩，反过来也行。

它是**预览**：Switch 上的客户端 [Ludonimbus-NX](https://github.com/Shupian0510/Ludonimbus-NX) 还在开发中，安装方法和进展见它的仓库。

## 怎样工作

- Switch 只在**局域网**里和你的电脑通信。云盘、存档历史和冲突都由电脑上的 Ludonimbus 处理，所以同步时电脑要开着 Ludonimbus。
- 在 Ludonimbus 里，Switch 是另一台设备，名字显示为「Switch」；它的存档和模拟器里的那个游戏在同一张游戏卡上。
- Switch 上的客户端读写的是系统里的存档本身，不需要 `prod.keys`。

## 在电脑上打开

1. 「设置 → Switch」打开「允许 Switch 连接」。这个开关默认是关的。
2. Windows 第一次问是否允许 Ludonimbus 访问网络时，选「专用网络」。
3. 点「配对新的 Switch」，电脑上显示一个配对码：在 Switch 上输入。配对码 5 分钟内有效，输错 5 次后失效。
4. 配对时在 Switch 上核对电脑显示的证书指纹。之后 Switch 只认这台电脑。

「设置 → Switch」里列出配对过的 Switch 和上次连接的时间，可以随时「移除」。

## 和其他版本

- **Switch 模拟器**：同一个游戏在主机、Ryujinx 和 Eden 之间自动共用进度。
- **PC 版**：存档格式通常不同，各存各的。规则列表里的游戏（例如星露谷物语）可以共用进度，见[同一个游戏的几个版本](../sync/game-versions.md#rule)。
- 在模拟器上用更新版本的游戏存的档，主机上的旧版本游戏可能读不了：先把主机上的游戏更新到同一个版本。
