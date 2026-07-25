# Vapourfly PRD

**产品名**：Vapourfly
**定位**：像 Spotify 歌单一样玩转 Steam 库的智能管理+发现工具
**核心理念**：像 Nike Vaporfly 一样轻盈快速飞越你的 Steam 游戏库（Vapour = 蒸汽，Fly = 飞一样浏览推荐和游戏单）

> **本文档取代原始 PRD。** 原始 PRD 描述的是 Python CLI + GUI 为非目标 + 写 localconfig.vdf 的早期构想。实际项目是 Rust workspace + egui GUI + 写 cloud-storage。本文档以代码和 grilling 决定为准。架构决策记录在 `docs/adr/`，领域语言定义在 `CONTEXT.md`。

## 问题与机会

- Steam 库越来越大，"喜加一"垃圾游戏泛滥，难以找到"好游戏"。
- Steam 自带收藏集/过滤器太基础，无法按个人游时、Deck 兼容、进度、心情智能分类。
- 玩家想像 Spotify 一样：创建游戏单、分享、匹配库、猜你喜欢、按时长推荐。
- 外部数据（HLTB 时长、ProtonDB Deck 评分、PCGW 手柄、RAWG/IGDB 评分）未被充分利用本地化。

## 目标用户

Steam 重度玩家（尤其是 Deck 用户、中国"喜加一"党），想要更好玩、更方便挑游戏、清理库、分享游戏单。

## 核心功能

### 1. Junk / 喜加一 智能识别与排除

三个信号（游时 / HLTB 主线时长 / 评分）+ 三种模式：

| 模式 | 逻辑 |
|---|---|
| **Default** | 游时低 + 至少一个其他信号低 + 至少 `min_available_signals` 个数据点 |
| **Strict** | 游时低 + 所有*可用*信号都低 + 至少 `min_available_signals` 个数据点 |
| **Aggressive** | 游时低 + 至少一个其他信号低，不要求数据点数 |

Default 是默认模式。每个判定可解释：matched 信号、missing 信号、confidence 分数。支持手动 override（force_include / force_exclude / 手动 HLTB / 手动评分）。可一键隐藏或建 Junk 收藏集。

> 原始 PRD 的"硬性 AND 三条件"模型已丢弃——HLTB/RAWG 覆盖率不全，硬 AND 判不出几个游戏。

### 2. 智能推荐 "今晚玩什么"

7 个加权信号的加法打分模型：

| 信号 | 权重 | 条件 |
|---|---|---|
| low_playtime | +2.0 | 游时 < 120min |
| deck_compatible | +2.0/+1.5/+1.0 | Native/Platinum/Gold（仅 deck 模式） |
| time_match | +1.5 | HLTB 主线 ≤ 可用时长 |
| high_rating | +1.0 | RAWG ≥4.0 或 IGDB ≥80 |
| taste_similarity | +1.0 | 口味向量重叠 >5% |
| recently_played_penalty | −1.0 | 14 天内玩过 |
| likely_finished_penalty | −0.5 | 游时 > 1.5× HLTB 主线 |

**权重固定不可调**——用户只调外部参数（可用时长、数量、deck 模式、seed、排除集合）。Junk 和 hidden 游戏在打分前过滤掉。可选 seed 让结果可复现。推荐可写入临时 Steam 收藏集 `vapourfly-picks`。

### 3. Spotify 式游戏单（Playlists）

- 创建/编辑 JSON 游戏单（`vapourfly.playlist.v1` schema）：Manual（AppID 列表）或 Rules（布尔表达式）
- 规则算子：`ProtonAtLeast`、`HltbMaxMinutes`、`ControllerSupportFull`、`PlaytimeBetween`、`RatingAtLeast`、`HasGenre`、`HasTag`、`Installed`、`NotJunk`、`NotHidden`、`And`、`Or`、`Not`
- 分享：导出 .json 或**紧凑二进制分享码**（`VF1:` 前缀 + 压缩二进制 payload，携带 content + name + description）
- 导入匹配：显示拥有率、已玩进度、缺失列表
- 补全花费：Steam Store 缓存数据计算当前总价
- **Discover**（猜你喜欢）：基于高游时游戏的口味向量相似度 + IGDB similar_games 生成推荐单，可选 seed AppID

> 分享码不向后兼容旧 base64url(JSON) 格式。见 [ADR-0003](docs/adr/0003-compact-binary-share-codes.md)。

### 4. 创意动态集合与编辑歌单

**透明动态模板**（用户可见规则）：
- **Deck Session**：Installed + NotHidden + NotJunk + ProtonAtLeast Gold + ControllerSupportFull + HltbMaxMinutes
- **Finish It**：游时在 HLTB 主线的 0.5–1.25 倍之间（快通关了）

**编辑歌单（Editorial Moods）**（命名策展，隐藏条件，类似 Spotify 编辑歌单）：

| 歌单名 | 隐藏条件 |
|---|---|
| Today's Biggest Hits | 库内近期热度 surge（打折/玩家涌入/近期评测活跃） |
| Indie Rising | 独立游戏 + 高评分 + 近期发行 |
| Friday Party | Steam Store 分类含 Co-op / Local Multiplayer / Party |
| Deck Guardians | ProtonDB Platinum/Gold + 手柄全支持 + 短 HLTB |
| Unopened Treasures | 未玩 + 高评分 + 非 junk |
| Weekend Marathon | 未玩 + 长 HLTB + 高评分 |
| Quick Round | 未玩 + 短 HLTB + 非 junk |

> 编辑歌单名是英文规范名，中文显示名属于本地化层。见 [ADR-0004](docs/adr/0004-editorial-mood-replaces-tag-filter.md)。

全部可同步到 Steam cloud-storage 收藏集。

> Playlist Radio 已删除，Discover 吸收了其全部场景。见 [ADR-0005](docs/adr/0005-discover-absorbs-playlist-radio.md)。

### 5. 数据集成与缓存

| 数据源 | 凭证 | 用途 |
|---|---|---|
| IGDB | 环境变量 | 类型、主题、关键词、评分、time-to-beat、相似游戏 |
| RAWG | 环境变量 | 类型、标签、评分 |
| ProtonDB | 无 | Deck 兼容 tier |
| PCGamingWiki | 无 | 手柄支持、修复链接 |
| HLTB | 无，编译时 `hltb_scrape` feature | 主线/完成时长 |
| Steam Store | 无 | App 详情、价格、平台支持、分类 |

**Hydration 契约**：工作流（junk/recommend/playlist/discover/mood）遇缓存缺失时**按需懒拉取**，`--offline` 是唯一禁网开关。拉失败降级为该游戏用已有数据评估，工作流永不出错。见 [ADR-0002](docs/adr/0002-lazy-hydration-with-degradation.md)。

### 6. 底层管理

- 跨平台路径检测（macOS/Linux/Windows）
- 写入安全模型：所有写操作需 `--dry-run` 或 `--confirm`，写入前自动备份，原子写入，Steam 运行时拒绝写入
- **写入面仅限 cloud-storage-namespace-1.json**，localconfig.vdf 永远只读。见 [ADR-0001](docs/adr/0001-cloud-storage-only-write-surface.md)
- CLI + GUI（egui）双界面，功能对等
- 本地 JSON 缓存 + 规则引擎

## 非目标

- 编辑 per-app Steam 设置（标签、启动项、手柄配置）——需要写 localconfig.vdf，超出范围
- Unowned games 成为一等实体——只在 playlist match 上下文显示缺失游戏和补全花费
- 在线多人分享平台
- 自动购买（只计算价格）

## 成功指标

- 用户能快速清理 Junk 并找到"好玩的"
- 创建/分享游戏单像 Spotify 一样上头
- 推荐准确率高（用户实际去玩的比例）
- Deck 用户反馈"终于有好用的便携推荐了"

## 技术栈

- **Rust**（edition 2024，MSRV 1.96）
- Workspace：`crates/core`（业务逻辑）+ `crates/api`（外部数据+缓存）+ `crates/cli`（命令行）+ `crates/gui`（egui 桌面 GUI）
- 纯文本 VDF 解析 + JSON 操作 cloud-storage 文件
- 可选 IGDB / RAWG 凭证（环境变量）
- 本地 JSON 缓存 + 规则引擎

## 名字来源

Vapourfly = Vapour（蒸汽） + Fly（像 Nike Vaporfly 一样轻盈快速飞越库和游戏单）。
三个音节，crisp 动态感，对 Vapour 玩 fly 文字游戏 spin-off，独特易记、有品牌力。
