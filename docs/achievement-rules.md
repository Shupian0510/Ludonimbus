# 写存档成就规则

给本身没有成就的游戏（重编译、移植、原生重制）定义成就：Ludonimbus 读游戏的存档，满足规则就算解锁。规则是 Ludonimbus 自己维护的，放在仓库里随应用发布（MIT）。设计背景见 [plan-achievements-coverage.md](plan-achievements-coverage.md) 第 3 节。

## 文件放在哪里

- `adapters/achievements/<适配器 key>.json`：一个游戏一个文件，文件名是 `adapters/*.yaml` 里这个游戏的 `key`（例如 `zelda-oot-longshot`）。
- 在 `crates/core/src/achievements/rules.rs` 的 `BUNDLED` 里加一行，把文件编进程序。
- 用户的游戏用了这个适配器，Ludonimbus 就自动关联这份规则（来源显示为「Ludonimbus」），存档的位置用适配器里的 `saves`。
- 调试时可以把同名文件放进 `%APPDATA%\Ludonimbus\achievements\rules\`，它优先于内置的，不用重新编译。改完在游戏的「成就」页点「刷新」。

## 格式

和 [TrophyNX](https://github.com/Imp3r1us/TrophyNX) 的游戏文件相同，多了中文字段。TrophyNX 是 GPL-2.0，**不要从它复制文字或规则**，自己写。

```json
{
  "name": "The Legend of Zelda: Ocarina of Time (Longshot)",
  "name_zh": "塞尔达传说：时之笛（Longshot）",
  "reader": "totk_progress",
  "save_files": ["progress.sav"],
  "achievements": [
    { "id": "master_sword", "name": "Blade of Evil's Bane", "desc": "Draw the Master Sword.",
      "name_zh": "退魔之剑", "desc_zh": "拔出大师之剑。",
      "rule": { "type": "flag", "name": "IsGet.Weapon_Sword_Master" } }
  ]
}
```

- `id`：发布后不要改（已解锁的记录按 id 对应）；删掉的成就不要复用它的 id。
- `save_files`：存档文件名，在适配器的存档文件夹里往下找（最多 6 层）。有几个存档槽时，**取最好的那个**：每个成就只要有一个槽满足就算，解锁时间取那个文件的修改时间。
- `name` / `desc` 是英文，`name_zh` / `desc_zh` 是简体中文，界面按语言选。物品名尽量和游戏里的叫法一致；成就标题用自己的话写。
- `hidden`（可选）：未解锁时隐藏说明。

### 读取器 `reader`

| 读取器 | 存档 | 规则里怎么指定值 |
|---|---|---|
| `json` | JSON 文本（可选 `xor_key` 先异或） | `path`：`a/b`、`list[3]`、`list[*]`、`list[Key=V]/Value`、`tags[=V]` |
| `byml` | 任天堂 BYML（大小端都行，可以有 0x10 字节前缀） | 同上 |
| `gamedata_hash` | BotW 的 `game_data.sav`：CRC32(名字) / 值，从 0x0C 开始 | `name`、`names`、`pattern` |
| `totk_progress` | TotK 引擎的 `progress.sav`：Murmur3(名字) / 值，开头是 `04 03 02 01`，从 0x28 读到 `MetaData.SaveTypeHash` | 同上；枚举值写文字（`"equals": "Clear"`） |
| `binary_le` | 固定偏移的二进制（可选 `magic` 校验开头） | `offset`、`offsets`、`size`（字节）、`max`（超过当作 0） |

### 规则 `rule.type`

| 类型 | 含义 |
|---|---|
| `flag` | 第一个值 ≥ `target`（默认 1）；有 `equals` 时比较是否相等 |
| `count` | 为真（或等于 `equals`）的值的个数 ≥ `target` |
| `sum` | 所有值相加 ≥ `target` |
| `launches` | 从 Ludonimbus 启动的次数 ≥ `target`（不读存档） |
| `playtime_hours` | 所有设备的游戏时间（小时）≥ `target`（不读存档） |

`pattern` + `from` / `to` 生成一串名字，例如 `"pattern": "DungeonState.Dungeon%03d", "from": 0, "to": 151`。

## 怎么找到存档里的含义

- **JSON / BYML 存档**：直接打开看字段名；存一次档前后对比，看哪个字段变了。
- **TotK 引擎的存档**（Longshot 的时之笛就是）：存档里只有名字的哈希。游戏 romfs 里的 `GameData/GameDataList.Product.*.byml.zs` 列出所有哈希和类型（`.zs` 是 zstd，字典在 `Pack/ZsDic.pack.zs` 里）。名字从游戏文件（AINB、事件、bgyml）和程序里的字符串收集，逐个算 Murmur3 去对；物品是否得到过一般是 `IsGet.<物品的 Actor 名>`，数量是 `PouchData.<Actor 名>.StockNum`。
- 用几个进度不同的存档槽核对：规则算出的结果要和游戏里看到的一致。
- 没有存档样本时不要写规则。只照着源码推出来的规则可能整份都对不上。

## 测试

- `cargo test -p ludonimbus-core rules`：`bundled_lists_are_well_formed` 检查每个内置文件能解析、`id` 不重复、有中英文、读取器和规则类型已知。新游戏最好再加一个用构造存档的测试（参考 `longshot_list_on_made_up_saves`）。
- 用你电脑上的真实存档（只读）：

  ```powershell
  cargo test -p ludonimbus-core --test achievements_live real_save_rules -- --ignored --nocapture
  ```

  列出配置里每个有规则的游戏、找到几个存档文件、解锁了哪些。

## 现有的规则

| 适配器 | 游戏 | 读取器 | 成就数 | 核对过的存档 |
|---|---|---|---|---|
| `zelda-oot-longshot` | 塞尔达传说：时之笛（Longshot） | `totk_progress` | 45 | 2026-11-24，6 个存档槽：解锁 13 个，和游戏进度一致 |

还没有「打败加侬」：原版时之笛在结局之后不存档，这里多半也一样，等有通关后的存档核对过再加。
