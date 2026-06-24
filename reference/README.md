# Reference Materials

从四个参考项目提取的源码和真实 Steam 文件样本，用于 Vapourfly 开发。所有外部依赖已自包含在此目录中。

## 目录结构

### `depressurizer/` — Steam 库分类管理器（C#/.NET）
来源：[rallion/depressurizer](https://github.com/rallion/depressurizer)（GPLv3）

| 文件 | 参考价值 |
|------|----------|
| `VdfFileNode.cs` | **核心参考** — 完整的 Text + Binary VDF 解析器/序列化器 |
| `GameData.cs` | Category/GameInfo/GameList 数据模型，Steam sharedconfig.vdf 导入导出 |
| `GameDB.cs` | Steam Store 页面爬取模式（URL、正则表达式） |
| `AutoCat.cs` | 自动分类引擎架构（Genre/Flags/Tags/Year/UserScore） |
| `Profile.cs` | Steam ID 64↔32 位转换，账户检测 |
| `Resources.Designer.cs` | Steam API URL 常量 |

### `steamtools/` — Watt Toolkit 核心库（C#/.NET）
来源：[BeyondDimension/SteamClient](https://github.com/BeyondDimension/SteamClient)

| 文件 | 参考价值 |
|------|----------|
| `BinaryReaderExtensions.SteamAppProperty.cs` | **核心参考** — appinfo.vdf 二进制 property table 解析（type bytes 0x00-0x08，V3 string pool） |
| `SteamKeyValue.cs` | 二进制 KV 解析器（Unicode strings，用于成就/统计 .bin 文件） |
| `VdfHelper.cs` | VDF 读写封装（text+binary 读取，始终 text 写入） |
| `ISteamService.Properties.cs` | **核心参考** — 跨平台 Steam 路径检测（macOS/Linux/Windows） |
| `SteamServiceImpl.cs` | **核心参考** — appinfo.vdf 解析流程（magic numbers、entry loop）、loginusers.vdf 读写、librarycache 图片路径 |
| `SteamServiceImpl.Abstract.cs` | macOS/Linux 下通过 registry.vdf 设置当前用户 |
| `SteamApp.cs` | SteamApp 完整数据模型（State 位掩码、属性提取、图片 URL 计算） |
| `SAM.API.Steam.cs` | Steam native 库加载器（steamclient.dylib/.dll/.so） |
| `SAM.API.Client.cs` | Steam 本地 IPC 客户端（CreateInterface、callback 循环） |
| `ISteamworksLocalApiService.cs` | Steamworks 本地 API 接口定义 |
| `SteamworksLocalApiServiceImpl.cs` | Steamworks 本地 API 实现（OwnsApps、成就、云存档） |

### `gameloop-vdf/` — VDF 文本解析库（C#/.NET）
来源：[BeyondDimension/Gameloop.Vdf](https://github.com/BeyondDimension/Gameloop.Vdf)（MIT）

| 文件 | 参考价值 |
|------|----------|
| `VdfTextReader.cs` | **核心参考** — 逐字符 tokenizer（引号、escape、注释、条件编译） |
| `VdfSerializer.cs` | 解析器编排（ReadProperty → ReadObject 递归） |
| `VdfTextWriter.cs` | 格式化 VDF 文本输出（缩进、引号） |
| `VdfStructure.cs` | 格式字符常量（Quote、Escape、ObjectStart/End 等） |
| `VdfConvert.cs` | 公共 API 入口（Deserialize/Serialize） |
| `VProperty.cs` / `VObject.cs` / `VValue.cs` | VDF 数据树节点类型 |

### `tinywii/` — Wii 备份管理器（Rust/Slint）
来源：[mq1/TinyWiiBackupManager](https://github.com/mq1/TinyWiiBackupManager)（GPL-3.0）

| 文件 | 参考价值 |
|------|----------|
| `workspace-Cargo.toml` | Rust workspace 结构（members、dependency 统一版本） |
| `core-Cargo.toml` | core crate 依赖配置 |
| `gui-Cargo.toml` | gui crate 依赖配置 |
| `core-lib.rs` | core 模块声明 |
| `game.rs` | Game 数据模型（GameID、搜索索引） |
| `config.rs` | JSON 序列化配置 |
| `covers.rs` | HTTP 下载封面图（ureq Agent 模式） |
| `data_dir.rs` | 跨平台数据目录（portable mode） |
| `gui-main.rs` | egui/Slint 入口、消息循环 |
| `state.rs` | UI 状态管理（State struct、update 方法） |
| `messages.slint` | Message enum + Dispatcher 模式 |
| `ui-state.slint` | 全局 UI 状态单例 |

### `steam-samples/` — 真实 Steam 文件样本
来源：本机 macOS Steam 安装（`~/Library/Application Support/Steam/`）

| 文件 | 用途 |
|------|------|
| `loginusers.vdf` | Text VDF — 登录账户列表（SteamID64 作 key） |
| `libraryfolders.vdf` | Text VDF — Steam 库文件夹定义 |
| `config.vdf` | Text VDF — Steam 客户端配置（CM 服务器、连接参数） |
| `registry.vdf` | Text VDF — macOS/Linux 下的注册表替代（AutoLoginUser 等） |
| `sharedconfig.vdf` | Text VDF — 用户漫游配置（本机几乎为空，已弃用） |
| `localconfig.vdf` | Text VDF — 用户本地配置（游时、好友、per-app 设置） |
| `cloud-storage-namespace-1.json` | JSON — **现代用户集合数据**（user-collections.* 条目） |
| `librarycache-730.json` | JSON — 单游戏库缓存（badge、成就、描述） |

## 关键格式速查

### Text VDF 语法
```
"key"		"value"
"parent"
{
	"child"		"123"
}
```

### Binary VDF 标记字节（shortcuts.vdf 风格）
```
0x00 = 子节点开始 + null-terminated key
0x01 = string 值 + null-terminated string
0x02 = int32 值 + 4 bytes LE
0x08 = 当前节点结束
```

### appinfo.vdf 二进制格式
```
[u32] magic: 0x07564427(V1) / 0x07564428(V2) / 0x07564429(V3)
[u32] universe
[V3: i64 string_table_offset → u32 count → null-terminated UTF-8 strings]
循环:
  [u32] app_id (0=end)
  [u32] data_length
  [data_length bytes]: 16B + 20B SHA1 + 4B changeNumber + property_table
Property Table:
  [u8] type: 0=Table, 1=String, 2=Int32, 3=Float, 5=WString, 6=Color, 7=Uint64, 8=End
  [null-terminated string or u16 index] name
  [type-specific data]
```

### user-collections JSON 格式
```json
["user-collections.xxx", {
  "key": "user-collections.xxx",
  "timestamp": 1234567890,
  "value": "{\"id\":\"xxx\",\"name\":\"...\",\"added\":[appid,...],\"removed\":[]}",
  "version": "123"
}]
```
