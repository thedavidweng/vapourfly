# Vapourfly PRD

**产品名**：Vapourfly  
**版本**：v0.2 开工版（2026-06-24）  
**定位**：像 Spotify 歌单一样管理、清理、发现 Steam 游戏库的本地优先工具。  
**开工状态**：绿灯，可进入 CLI-first 工程骨架与垂直切片开发。

## 1. 产品目标

Vapourfly 面向 Steam 重度玩家、Steam Deck 用户和“喜加一”游戏库用户，解决三个问题：

- 从大库里快速找出适合当下时间、设备和心情的游戏。
- 自动识别低价值/低意愿游戏，集中放入 Junk 集合并从推荐中排除。
- 像 Spotify 歌单一样创建、导入、分享、匹配和扩展游戏单。

## 2. MVP 范围

MVP 聚焦本地 Steam 库扫描、Junk 识别、推荐和 Steam 用户集合同步。

### 2.1 Steam 本地数据

- 自动检测 Steam 安装目录和 `userdata/{uid}` 账户。
- 读取 `loginusers.vdf`、`libraryfolders.vdf`、`appmanifest_*.acf`、`librarycache/*.json`。
- 读取 `localconfig.vdf` 的 `apps.{appid}.playtime`、`LastPlayed`、`Playtime2wks`、`PlaytimeDisconnected`。
- 读取 `cloud-storage-namespace-1.json` 的 `user-collections.*`。
- 隐藏状态使用 `user-collections.hidden`，集合写入使用 `cloud-storage-namespace-1.json`。
- `localconfig.vdf` 只用于游时、最后游玩和 per-app 设置读取。

### 2.2 Junk / 喜加一识别

默认规则：

- `playtime < 30min`
- 通关时长短：优先 IGDB `game_time_to_beats.hastily/normally`，其次 HLTB 缓存，缺失时跳过该条件。
- 评分低：优先 RAWG rating，次选 IGDB rating/total_rating，缺失时跳过该条件。

命中全部可用条件后标记为 Junk。缺关键数据时采用保守策略，避免误标记。

MVP 操作：

- `junk preview`：只输出候选列表、命中原因、数据来源。
- `junk apply --dry-run`：生成写入差异。
- `junk apply --confirm`：写入 Vapourfly Junk 集合。
- `junk hide --confirm`：把 Junk AppID 加入 `user-collections.hidden`。

### 2.3 “今晚玩什么”推荐

输入可用时间、设备偏好和过滤条件，输出 5 个加权推荐：

- 未玩/低游时加权。
- 排除 Junk 和 hidden。
- Steam Deck 场景优先 ProtonDB `native/platinum/gold`。
- 时长匹配优先 IGDB/HLTB。
- 评分和类型相似度优先 RAWG/IGDB。
- 可生成临时 Steam 集合 `vapourfly-tonight-{date}`。

### 2.4 Spotify 式游戏单

- 创建手动游戏单：名称、描述、AppID 列表。
- 创建规则游戏单：例如 `proton>=gold + time<=5h + playtime=0 + not_junk`。
- 导出/导入 `.vapourfly-playlist.json`。
- 导入后显示拥有率、已玩进度、缺失列表。
- 使用 Steam Store `appdetails` 估算缺失游戏当前补全花费。
- Discover/Radio 使用 IGDB `similar_games`、genres、themes、keywords，RAWG tags 作为补充。

### 2.5 外部数据源

| 数据源 | MVP 作用 | 鉴权 | 失败策略 |
|---|---|---|---|
| Steam Store appdetails | 名称、价格、商店信息 | 无 | 使用本地名称，价格显示未知 |
| Steam AppList | AppID 名称映射 | 无 | 使用 librarycache/appmanifest 名称 |
| IGDB | 类型、相似游戏、评分、通关时间、Steam AppID 映射 | Twitch Client ID/Secret | 跳过 IGDB 加权，继续推荐 |
| RAWG | 评分、genres、tags、stores | API key | 使用 IGDB 评分/类型补位 |
| ProtonDB | Steam Deck/Linux 兼容 tier | 无官方稳定承诺 | 显示 unknown，推荐降低权重 |
| PCGamingWiki | 手柄支持、修复提示、技术特性 | MediaWiki/Cargo API | 显示 unknown |
| HLTB | 通关时长补充 | 无官方 API | 作为可选模块，缓存优先 |

## 3. 非目标

- 在线多人分享平台。
- Steam 账号登录、交易、库存或好友系统。
- 修改 Steam 二进制文件、成就、云存档。
- 依赖 Steamworks IPC 的核心流程。
- 自动删除游戏、卸载游戏或更改商店购买状态。

## 4. 技术栈

- Rust 2024 edition。
- CLI-first：`core + api + cli` 先落地，`gui` 后置。
- GUI：`egui/eframe`。
- 数据存储：本地 JSON + Steam 原生配置文件。
- 网络：统一 HTTP client、缓存、限速、重试。
- 安全写入：默认只读；写入必须 `--dry-run` 预览，`--confirm` 执行。

## 5. 成功指标

MVP 满足以下条件即达成：

- 能在 macOS/Linux/Windows 检测 Steam 路径和至少一个用户。
- 能扫描本地库并输出 AppID、名称、安装状态、游时、最后游玩时间、所属集合、hidden 状态。
- 能预览 Junk 候选并解释命中原因。
- 能生成推荐列表并标注每个推荐的来源权重。
- 能把 Vapourfly 集合同步进 `cloud-storage-namespace-1.json`。
- 写入前自动备份，写入后可验证，失败可回滚。
- 外部 API 全失败时仍可完成本地扫描和基础推荐。

## 6. 开发阶段

### Phase 0：文档与样本锁定

- 锁定 Steam 文件读写边界。
- 锁定 API 鉴权、缓存、限速、失败策略。
- 锁定许可证和参考源码使用边界。
- 用 `reference/steam-samples/` 建立固定测试样本。

### Phase 1：CLI 垂直切片

- 初始化 workspace：`core`、`api`、`cli`。
- 实现 `vapourfly doctor`、`scan`、`collections list`。
- 读取 `localconfig.vdf` 游时和 `cloud-storage-namespace-1.json` 集合。
- 输出只读 JSON 结果。

### Phase 2：Junk 与安全写入

- 实现 `junk preview`、`junk apply --dry-run`、`sync --dry-run`。
- 实现 backup、atomic write、rollback、diff。
- 写入 Vapourfly Junk 集合和 hidden 集合。

### Phase 3：外部 API 与缓存

- 接入 IGDB、RAWG、ProtonDB、PCGamingWiki、Steam Store。
- HLTB 作为可选增强模块。
- 实现离线缓存、限速、429 重试、缓存刷新。

### Phase 4：推荐和游戏单

- 实现 `recommend --minutes`。
- 实现 playlist import/export/match/radio。
- 实现补全花费估算。

### Phase 5：GUI

- 基于 core/cli 已验证能力构建 egui 界面。
- 展示库、Junk、推荐、游戏单、同步状态。

## 7. 开工门槛

当前文档已满足开工门槛。实现前仍需开发者在本地提供可选 API 凭据：

- `VAPOURFLY_IGDB_CLIENT_ID`
- `VAPOURFLY_IGDB_CLIENT_SECRET`
- `VAPOURFLY_RAWG_KEY`

缺少凭据时功能降级运行。
