# Vapourfly 技术实现方案

**版本**：v0.2 开工版（2026-06-24）  
**状态**：可直接进入 CLI-first 工程实现。  
**范围**：Steam 本地库扫描、集合读写、Junk 检测、推荐、游戏单、外部 API 缓存与 GUI 后续接入。

> 外部参考源码和 Steam 文件样本在 `reference/`。参考源码只用于理解格式和设计思路；Vapourfly 的生产实现采用独立 Rust 实现。

---

## 0. 已消除的冲突

| 原问题 | 当前决策 |
|---|---|
| `epui` 与 `egui/eframe` 冲突 | 统一为 `egui/eframe` |
| PRD 写 `localconfig.vdf` 收藏集 | 集合统一读写 `cloud-storage-namespace-1.json` 的 `user-collections.*` |
| CLI 在路线图末尾 | CLI-first，GUI 后置 |
| IGDB 缺技术方案 | 新增 Twitch OAuth、IGDB client、Steam AppID 映射、缓存、限速、模型 |
| Junk 隐藏缺方案 | 使用 `user-collections.hidden` |
| playtime 数据源缺口 | 从 `localconfig.vdf` 的 `apps` 读取 `playtime`、`LastPlayed`、`Playtime2wks` |
| `CloudEntry.key` 内外键不一致 | 强制外层 key 与 entry.key 都等于 `user-collections.{id}` |
| GPL 参考边界模糊 | 新增 clean-room 和第三方声明策略 |
| 外部 API 缺失败策略 | 所有 API 支持缓存、限速、失败降级 |

---

## 1. Workspace 架构

```text
vapourfly/
├── Cargo.toml
├── crates/
│   ├── core/                  # 纯业务逻辑，无 UI，无 HTTP
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── game.rs
│   │       ├── playlist.rs
│   │       ├── junk.rs
│   │       ├── recommend.rs
│   │       ├── cache.rs
│   │       ├── config.rs
│   │       ├── license.rs
│   │       └── steam/
│   │           ├── mod.rs
│   │           ├── paths.rs
│   │           ├── vdf_text.rs
│   │           ├── vdf_binary.rs
│   │           ├── appinfo.rs
│   │           ├── account.rs
│   │           ├── library.rs
│   │           ├── localconfig.rs
│   │           ├── collections.rs
│   │           ├── write_plan.rs
│   │           └── backup.rs
│   ├── api/                   # 外部 API 客户端
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── http.rs
│   │       ├── steam_store.rs
│   │       ├── igdb.rs
│   │       ├── rawg.rs
│   │       ├── protondb.rs
│   │       ├── pcgw.rs
│   │       └── hltb.rs
│   ├── cli/                   # 开工优先入口
│   │   └── src/main.rs
│   └── gui/                   # Phase 5
│       └── src/main.rs
├── data/fixtures/             # 从 reference/steam-samples 派生的测试夹具
├── THIRD_PARTY_NOTICES.md
└── IMPLEMENTATION_GATES.md
```

依赖方向：`cli -> core + api`，`gui -> core + api`，`api -> core/cache/config`，`core` 只依赖标准库、serde、chrono、thiserror 等基础库。

---

## 2. 数据模型

### 2.1 Game

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Game {
    pub app_id: u32,
    pub name: String,
    pub app_type: SteamAppType,
    pub installed: bool,
    pub install_dir: Option<PathBuf>,
    pub library_folder: Option<PathBuf>,

    // localconfig.vdf / appmanifest / librarycache
    pub playtime_minutes: Option<u32>,
    pub playtime_2wks_minutes: Option<u32>,
    pub playtime_disconnected_minutes: Option<u32>,
    pub last_played_unix: Option<i64>,

    // Steam collection state
    pub steam_collections: Vec<String>,
    pub is_hidden: bool,
    pub is_junk: bool,

    // External data
    pub hltb: Option<HltbData>,
    pub igdb: Option<IgdbData>,
    pub rawg: Option<RawgData>,
    pub protondb: Option<ProtonDbData>,
    pub pcgw: Option<PcgwData>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SteamAppType {
    Game,
    Application,
    Tool,
    Demo,
    Dlc,
    Unknown(String),
}
```

### 2.2 外部数据模型

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HltbData {
    pub main_story_seconds: Option<u32>,
    pub main_extra_seconds: Option<u32>,
    pub completionist_seconds: Option<u32>,
    pub source: HltbSource,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum HltbSource {
    IgdbGameTimeToBeat,
    HltbScrape,
    ManualOverride,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IgdbData {
    pub igdb_id: u64,
    pub name: String,
    pub slug: Option<String>,
    pub rating_0_100: Option<f32>,
    pub total_rating_0_100: Option<f32>,
    pub genres: Vec<String>,
    pub themes: Vec<String>,
    pub keywords: Vec<String>,
    pub similar_game_ids: Vec<u64>,
    pub steam_app_id_confirmed: bool,
    pub time_to_beat: Option<IgdbTimeToBeat>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IgdbTimeToBeat {
    pub hastily_seconds: Option<u32>,
    pub normally_seconds: Option<u32>,
    pub completely_seconds: Option<u32>,
    pub submission_count: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawgData {
    pub rawg_id: u64,
    pub rating_0_5: Option<f32>,
    pub ratings_count: Option<u32>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub stores: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProtonDbData {
    pub tier: ProtonTier,
    pub confidence: Option<String>,
    pub score: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProtonTier {
    Borked,
    Bronze,
    Silver,
    Gold,
    Platinum,
    Native,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PcgwData {
    pub page_name: Option<String>,
    pub controller_support: ControllerSupport,
    pub steam_deck_notes: Option<String>,
    pub fixes_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ControllerSupport {
    Full,
    Partial,
    None,
    Unknown,
}
```

---

## 3. Steam 文件交互

### 3.1 文件职责

| 文件 | 路径 | 读取 | 写入 | 用途 |
|---|---|---:|---:|---|
| `loginusers.vdf` | `{steam}/config/` | 是 | 否 | 账户检测、最近用户 |
| `libraryfolders.vdf` | `{steam}/config/` | 是 | 否 | Steam 库目录 |
| `appmanifest_*.acf` | `{library}/steamapps/` | 是 | 否 | 已安装 AppID、installdir、StateFlags |
| `librarycache/*.json` | `{steam}/userdata/{uid}/config/librarycache/` 和 `{steam}/appcache/librarycache/` | 是 | 否 | 库显示数据和图片信息 |
| `localconfig.vdf` | `{steam}/userdata/{uid}/config/` | 是 | 否 | 游时、最后游玩、2 周游时、per-app 设置 |
| `cloud-storage-namespace-1.json` | `{steam}/userdata/{uid}/config/cloudstorage/` | 是 | 是 | 用户集合、hidden 集合 |
| `sharedconfig.vdf` | `{steam}/userdata/{uid}/7/remote/` | 是 | 否 | 旧版兼容，只读检测 |
| `shortcuts.vdf` | `{steam}/userdata/{uid}/config/` | 后续 | 否 | 非 Steam 游戏，MVP 后置 |
| `appinfo.vdf` | `{steam}/appcache/` | 可选 | 否 | App 元数据缓存，缺失时正常降级 |

集合和隐藏只写 `cloud-storage-namespace-1.json`。`localconfig.vdf` 保持只读。

### 3.2 跨平台路径检测

```rust
pub fn detect_steam_dirs() -> Vec<PathBuf> {
    // macOS:
    //   ~/Library/Application Support/Steam
    // Linux:
    //   ~/.steam/steam
    //   ~/.local/share/Steam
    // Steam Deck:
    //   ~/.steam/steam
    // Windows:
    //   HKCU\Software\Valve\Steam\SteamPath
    //   C:\Program Files (x86)\Steam
}

pub fn detect_accounts(steam_dir: &Path) -> Result<Vec<SteamAccount>>;
pub fn detect_library_folders(steam_dir: &Path) -> Result<Vec<PathBuf>>;
```

账户选择规则：

1. `loginusers.vdf` 中 `mostrecent = 1` 优先。
2. 单账户自动选择。
3. 多账户 CLI 要求 `--account <steamid32|steamid64|account_name>`。
4. GUI 显示账户选择器。

### 3.3 Text VDF 解析

MVP 必须支持：

- quoted key/value。
- 未引号 token。
- `{}` 递归对象。
- `//` 行注释。
- 常见 escape：`\n`、`\t`、`\r`、`\\`、`\"`。
- 平台条件 token 保留原文。
- 重复 key 以 `Vec<(String, VdfNode)>` 保序存储，避免 Steam 文件重排造成风险。

```rust
pub enum VdfNode {
    Object(Vec<(String, VdfNode)>),
    String(String),
}

pub fn parse_text_vdf(input: &str) -> Result<VdfNode>;
pub fn write_text_vdf(node: &VdfNode) -> String;
```

### 3.4 localconfig.vdf 游时解析

样本路径：

```text
UserLocalConfigStore
  Software
    Valve
      Steam
        apps
          {appid}
            LastPlayed        "1628871494"
            playtime          "1038"
            Playtime2wks      "213"
            PlaytimeDisconnected "3"
```

实现：

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalAppState {
    pub app_id: u32,
    pub last_played_unix: Option<i64>,
    pub playtime_minutes: Option<u32>,
    pub playtime_2wks_minutes: Option<u32>,
    pub playtime_disconnected_minutes: Option<u32>,
    pub raw_fields: BTreeMap<String, String>,
}

pub fn parse_localconfig_apps(path: &Path) -> Result<BTreeMap<u32, LocalAppState>> {
    // 1. parse_text_vdf
    // 2. descend UserLocalConfigStore/Software/Valve/Steam/apps
    // 3. app key parse u32
    // 4. read exact field names with case handling:
    //    playtime, LastPlayed, Playtime2wks, PlaytimeDisconnected
    // 5. unknown fields preserved in raw_fields
}
```

验收：使用 `reference/steam-samples/localconfig.vdf`，AppID `70` 解析出 `playtime=418`、`Playtime2wks=213`。

### 3.5 cloud-storage-namespace-1.json 数据结构

样本是数组，每个元素为 `[outer_key, entry]`。

```rust
pub type CloudStorageFile = Vec<(String, CloudEntry)>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloudEntry {
    pub key: String,
    pub timestamp: Option<i64>,
    pub value: Option<String>,
    pub version: Option<String>,

    #[serde(default)]
    pub is_deleted: Option<bool>,

    #[serde(default, rename = "conflictResolutionMethod")]
    pub conflict_resolution_method: Option<String>,

    #[serde(default, rename = "strMethodId")]
    pub str_method_id: Option<String>,

    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollectionValue {
    pub id: String,
    pub name: String,
    pub added: Vec<u32>,
    pub removed: Vec<u32>,

    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SteamCollection {
    pub id: String,
    pub name: String,
    pub app_ids: Vec<u32>,
    pub removed_app_ids: Vec<u32>,
    pub is_hidden_collection: bool,
}
```

读取规则：

```rust
pub fn read_user_collections(path: &Path) -> Result<Vec<SteamCollection>> {
    // 1. serde_json parse Vec<(String, CloudEntry)>
    // 2. outer_key starts_with "user-collections."
    // 3. entry.is_deleted == true => skip
    // 4. entry.value must exist and parse CollectionValue
    // 5. effective app_ids = added - removed
    // 6. is_hidden_collection = id == "hidden" or outer_key == "user-collections.hidden"
}
```

写入规则：

- 外层 key：`user-collections.{id}`。
- `CloudEntry.key`：必须等于同一个完整 key。
- `CollectionValue.id`：只保存 `{id}`。
- `CollectionValue.name`：用户可见名称。
- `added` 全量保存目标 AppID 列表。
- `removed` 默认空数组。
- 保留旧 entry 的 `version`、`conflictResolutionMethod`、`strMethodId`、`extra`。
- 新 entry 的 `version` 留空，由 Steam 后续处理。
- 写入时按 AppID 升序去重。

```rust
pub fn upsert_collection(
    raw: &mut CloudStorageFile,
    collection: &SteamCollection,
    now_unix: i64,
) -> Result<()> {
    let outer_key = format!("user-collections.{}", collection.id);
    let value = CollectionValue {
        id: collection.id.clone(),
        name: collection.name.clone(),
        added: sorted_unique(collection.app_ids.clone()),
        removed: vec![],
        extra: BTreeMap::new(),
    };
    let value_json = serde_json::to_string(&value)?;

    match raw.iter_mut().find(|(k, _)| k == &outer_key) {
        Some((_, entry)) => {
            entry.key = outer_key;
            entry.value = Some(value_json);
            entry.timestamp = Some(now_unix);
            entry.is_deleted = Some(false);
        }
        None => raw.push((outer_key.clone(), CloudEntry {
            key: outer_key,
            timestamp: Some(now_unix),
            value: Some(value_json),
            version: None,
            is_deleted: Some(false),
            conflict_resolution_method: None,
            str_method_id: None,
            extra: BTreeMap::new(),
        })),
    }
    Ok(())
}
```

### 3.6 hidden 集合策略

Steam hidden 通过集合实现：

```text
outer key: user-collections.hidden
entry.key: user-collections.hidden
value: {"id":"hidden","name":"Hidden","added":[...],"removed":[]}
```

Junk 隐藏流程：

1. 读取现有 hidden 集合。
2. 合并 Junk AppID。
3. 生成 `WritePlan` diff。
4. `--dry-run` 输出新增数量和 AppID。
5. `--confirm` 写入。

### 3.7 安全写入流水线

```rust
pub struct WritePlan {
    pub target_path: PathBuf,
    pub backup_path: PathBuf,
    pub tmp_path: PathBuf,
    pub before_sha256: String,
    pub after_sha256: String,
    pub operations: Vec<WriteOp>,
}

pub enum WriteOp {
    UpsertCollection { id: String, added: Vec<u32>, removed: Vec<u32> },
    AddToHidden { app_ids: Vec<u32> },
}
```

执行顺序：

1. 检查目标文件存在且可读。
2. 读取并解析目标文件。
3. 生成内存 diff。
4. `--dry-run` 只打印 diff 和目标路径。
5. `--confirm` 进入写入。
6. 检测 Steam 进程：运行中时给出强提示，CLI 需要 `--allow-steam-running` 才继续。
7. 创建备份：`cloud-storage-namespace-1.json.vapourfly-backup-{timestamp}.json`。
8. 同目录写临时文件：`.cloud-storage-namespace-1.json.vapourfly.tmp`。
9. 写入 pretty JSON，flush，fsync。
10. rename 覆盖目标文件。
11. 重新读取并验证 JSON 可解析、目标集合存在、AppID 数量一致。
12. 验证失败则恢复备份。
13. 保留最近 5 份备份。

---

## 4. 外部 API 集成

### 4.1 HTTP 与缓存基础设施

```rust
pub struct HttpClient {
    agent: ureq::Agent,
    limiter: RateLimiter,
    cache: Cache,
}

pub struct CacheRecord<T> {
    pub source: String,
    pub key: String,
    pub fetched_at_unix: i64,
    pub ttl_seconds: u64,
    pub data: T,
    pub etag: Option<String>,
    pub source_version: Option<String>,
}
```

缓存目录：

```text
{app_data}/vapourfly/cache/
  steam/applist.json
  steam_store/{appid}.json
  igdb/auth_token.json          # 0600 权限，保存 access_token 与 expires_at
  igdb/games_by_steam/{appid}.json
  igdb/games/{igdb_id}.json
  igdb/time_to_beat/{igdb_id}.json
  rawg/{appid}.json
  protondb/{appid}.json
  pcgw/{appid}.json
  hltb/{appid}.json
```

全局策略：

- 网络超时：10 秒。
- 429：指数退避，最多 3 次。
- 5xx：使用 stale cache，标记 `data_freshness = stale`。
- key 缺失：该数据源跳过。
- UI/CLI 显示每个字段来源。

### 4.2 Steam Store / AppList

```rust
pub async fn fetch_applist() -> Result<HashMap<u32, String>>;
pub async fn fetch_appdetails(app_id: u32, cc: &str, lang: &str) -> Result<SteamStoreDetails>;
```

端点：

- `https://api.steampowered.com/ISteamApps/GetAppList/v0002/`
- `https://store.steampowered.com/api/appdetails?appids={appid}&cc={cc}&l={lang}`

缓存：

- AppList：7 天。
- AppDetails：30 天。
- 价格字段：24 小时。

### 4.3 IGDB 完整方案

IGDB 使用 Twitch 开发者凭据。官方流程要求创建 Twitch 应用、生成 Client ID 和 Client Secret，通过 `client_credentials` 获取 access token；API 请求使用 `POST https://api.igdb.com/v4/{endpoint}`，Header 包含 `Client-ID` 和 `Authorization: Bearer {token}`；请求体使用 APICalypse 查询语法。官方限速为 4 requests/s，最多 8 个并发请求。

配置：

```text
VAPOURFLY_IGDB_CLIENT_ID
VAPOURFLY_IGDB_CLIENT_SECRET
```

Token 获取：

```http
POST https://id.twitch.tv/oauth2/token
  ?client_id={client_id}
  &client_secret={client_secret}
  &grant_type=client_credentials
```

Token 缓存：

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IgdbTokenCache {
    pub access_token: String,
    pub token_type: String,
    pub expires_at_unix: i64,
}
```

实现要求：

- token 文件权限在 Unix 设置为 `0600`。
- `expires_at_unix - now < 3600` 时刷新。
- client secret 只从环境变量或 OS keychain 读取，禁止写入普通配置文件。

限速：

```rust
pub const IGDB_MAX_RPS: u32 = 3;          // 留出安全余量，官方上限 4 rps
pub const IGDB_MAX_CONCURRENT: usize = 6; // 留出安全余量，官方上限 8 并发
```

Steam AppID → IGDB ID 映射：

1. 首次启动或缓存缺失时查询 `external_game_sources`，找到名称为 Steam/steam 的 source id 并缓存。
2. 查询 `external_games`：`uid = "{appid}" & external_game_source = {steam_source_id}`。
3. 兼容旧字段：当 `external_game_source` 结果缺失时允许查询 deprecated `category = 1`，仅作为 fallback。
4. 取得 `game` 字段后查询 `games` 详情。
5. 名称相似度低于阈值时标记 `steam_app_id_confirmed = false`，推荐算法降低权重。

```rust
pub async fn resolve_igdb_game_by_steam_appid(app_id: u32) -> Result<Option<IgdbGameRef>> {
    let steam_source_id = get_or_fetch_steam_external_source_id().await?;
    let query = format!(
        "fields game,name,uid,external_game_source,url; where uid = \"{}\" & external_game_source = {}; limit 10;",
        app_id, steam_source_id
    );
    post_igdb("external_games", &query).await
}
```

Game 详情查询：

```apicalypse
fields
  id,name,slug,rating,total_rating,genres.name,themes.name,keywords.name,
  similar_games,external_games,first_release_date,game_type,url;
where id = {igdb_id};
limit 1;
```

搜索 fallback：

```apicalypse
search "{steam_name}";
fields id,name,slug,rating,total_rating,genres.name,themes.name,keywords.name,similar_games,first_release_date;
where version_parent = null;
limit 10;
```

Time-to-beat 查询：

```apicalypse
fields game_id,hastily,normally,completely,count,updated_at;
where game_id = {igdb_id};
limit 1;
```

字段单位：

- `game_time_to_beats.hastily`：秒，快速通关到 credits。
- `normally`：秒，主线+部分支线。
- `completely`：秒，100% 完成。
- `rating` 和 `total_rating`：0-100。

Vapourfly 映射：

```rust
pub fn igdb_to_hltb(data: &IgdbTimeToBeat) -> HltbData {
    HltbData {
        main_story_seconds: data.hastily_seconds.or(data.normally_seconds),
        main_extra_seconds: data.normally_seconds,
        completionist_seconds: data.completely_seconds,
        source: HltbSource::IgdbGameTimeToBeat,
    }
}

pub fn igdb_rating_to_rawg_scale(rating_0_100: f32) -> f32 {
    (rating_0_100 / 20.0).clamp(0.0, 5.0)
}
```

Discover/Radio 使用：

- `similar_games` 作为第一推荐来源。
- `genres + themes + keywords` 建立标签向量。
- Steam 库中已拥有游戏按游时加权形成用户偏好向量。
- 推荐只输出用户拥有游戏，商店发现后续版本再扩展到未拥有游戏。

### 4.4 RAWG 方案

配置：

```text
VAPOURFLY_RAWG_KEY
```

查询：

```http
GET https://api.rawg.io/api/games?key={key}&search={name}&stores=1&page_size=10
GET https://api.rawg.io/api/games/{rawg_id}?key={key}
```

匹配策略：

1. 用 Steam 名称搜索。
2. 优先 `stores` 包含 Steam 的结果。
3. 名称标准化相似度排序。
4. AppID 可从 RAWG stores 数据验证时标记 `steam_app_id_confirmed = true`。
5. 评分、genres、tags 缓存到 `rawg/{appid}.json`。

缓存：30 天。Key 缺失时跳过。

### 4.5 ProtonDB 方案

查询：

```http
GET https://www.protondb.com/api/v1/reports/summaries/{appid}.json
```

解析：

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProtonDbSummary {
    pub tier: Option<String>,
    pub confidence: Option<String>,
    pub score: Option<f32>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
```

策略：

- 端点作为社区数据源处理。
- HTTP 404/empty：tier = Unknown。
- 30 天缓存。
- CLI `sources status` 展示 ProtonDB 最近成功时间。

### 4.6 PCGamingWiki 方案

PCGamingWiki 基于 MediaWiki，官方 API 页面说明可使用 MediaWiki API 以及 Cargo API actions。Vapourfly 使用两步策略：

1. Steam AppID 页面解析：

```http
GET https://www.pcgamingwiki.com/api/appid.php?appid={appid}
```

返回跳转时提取最终页面名或 URL。

2. MediaWiki/Cargo 查询：

```http
GET https://www.pcgamingwiki.com/w/api.php?action=cargoquery&format=json&tables={table}&fields={fields}&where={page_field}%3D%22{page_name}%22
```

MVP 字段目标：

- 控制器支持：full/partial/none/unknown。
- Steam Deck/Linux 备注：文本摘要。
- Fixes URL：PCGW 页面 URL。

实现策略：

- `pcgw.rs` 将 Cargo table/field 放入可配置映射，首次实现以 fixtures 锁定。
- Schema 变化时保留 raw JSON 并降级 unknown。
- 缓存 30 天。
- 输出来源 URL，方便人工核验。

### 4.7 HLTB 方案

HLTB 没有官方公开 API。Vapourfly 将其设计为可选增强：

```toml
[features]
hltb_scrape = []
```

默认策略：

- 优先 IGDB `game_time_to_beats`。
- 用户启用 `hltb_scrape` 后才尝试 HLTB HTML/第三方兼容解析。
- 解析失败不影响 Junk、推荐、游戏单。
- 用户可通过 `~/.config/vapourfly/manual_overrides.json` 手工覆盖通关时长。

```rust
pub struct ManualOverride {
    pub app_id: u32,
    pub hltb_main_story_seconds: Option<u32>,
    pub hltb_main_extra_seconds: Option<u32>,
    pub hltb_completionist_seconds: Option<u32>,
}
```

---

## 5. Junk 检测

```rust
pub struct JunkRules {
    pub max_playtime_minutes: u32,      // default 30
    pub max_main_story_seconds: u32,    // default 7200
    pub max_rating_0_5: f32,            // default 2.5
    pub min_available_signals: usize,   // default 2
}

pub struct JunkDecision {
    pub app_id: u32,
    pub is_junk: bool,
    pub matched: Vec<JunkSignal>,
    pub missing: Vec<JunkSignalKind>,
    pub confidence: f32,
}

pub enum JunkSignal {
    LowPlaytime { minutes: u32 },
    ShortCompletion { seconds: u32, source: HltbSource },
    LowRating { rating_0_5: f32, source: RatingSource },
}
```

决策规则：

- `playtime` 必须命中。
- 时长和评分至少命中一个。
- `min_available_signals` 默认 2。
- 缺数据时显示 missing，候选不会自动进入 Junk。
- `--aggressive` 可要求 playtime + 任一负面信号。
- `--strict` 要求三项全部命中。

CLI：

```bash
vapourfly junk preview --format table
vapourfly junk preview --format json > junk.json
vapourfly junk apply --collection vapourfly-junk --dry-run
vapourfly junk apply --collection vapourfly-junk --confirm
vapourfly junk hide --confirm
```

---

## 6. 推荐引擎

```rust
pub struct RecommendRequest {
    pub available_minutes: u32,
    pub count: usize,
    pub deck_mode: bool,
    pub include_installed_only: bool,
    pub seed_playlist: Option<String>,
    pub exclude_collections: Vec<String>,
}

pub struct Recommendation {
    pub app_id: u32,
    pub name: String,
    pub score: f32,
    pub reasons: Vec<RecommendReason>,
}
```

评分：

| 信号 | 权重 |
|---|---:|
| 未玩过或低游时 | +2.0 |
| 非 Junk/非 hidden | 必须满足 |
| Deck mode + ProtonDB native/platinum/gold | +2.0 / +1.5 / +1.0 |
| 可用时间匹配 IGDB/HLTB | +1.5 |
| RAWG rating >= 4.0 或 IGDB total_rating >= 80 | +1.0 |
| 与高游时游戏 genre/theme/tag 相似 | +1.0 |
| 最近 14 天已玩 | -1.0 |
| 已通关推定 | -0.5 |

算法：

1. 过滤：hidden、Junk、非游戏类型、缺库记录。
2. 计算硬条件：Deck mode、安装状态、可用时长。
3. 计算分数。
4. 加入随机扰动 `0.0..0.25`，避免结果固定。
5. 返回 top N，并保留 reasons。

---

## 7. Playlist / 游戏单

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaylistFile {
    pub vapourfly_schema: String,       // "vapourfly.playlist.v1"
    pub created_by: String,             // "Vapourfly 0.2"
    pub playlist: Playlist,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub description: String,
    pub content: PlaylistContent,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum PlaylistContent {
    Manual { app_ids: Vec<u32> },
    Rules { rules: Vec<PlaylistRule> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", content = "args")]
pub enum PlaylistRule {
    ProtonAtLeast { tier: ProtonTier },
    HltbMaxMinutes { minutes: u32 },
    PlaytimeBetween { min: u32, max: u32 },
    RatingAtLeast { rating_0_5: f32 },
    HasGenre { genre: String },
    HasTag { tag: String },
    Installed,
    NotJunk,
    NotHidden,
    And(Vec<PlaylistRule>),
    Or(Vec<PlaylistRule>),
    Not(Box<PlaylistRule>),
}
```

导入匹配：

```rust
pub struct PlaylistMatchReport {
    pub owned: Vec<u32>,
    pub missing: Vec<u32>,
    pub played: Vec<u32>,
    pub unplayed: Vec<u32>,
    pub completion_price: Option<Money>,
}
```

Steam 同步：游戏单可导出为 `user-collections.vapourfly-{slug}`。

---

## 8. CLI 命令

```bash
vapourfly doctor
vapourfly scan --format table|json
vapourfly accounts list
vapourfly collections list
vapourfly collections export --out collections.json
vapourfly junk preview [--strict|--aggressive]
vapourfly junk apply --collection vapourfly-junk --dry-run
vapourfly junk apply --collection vapourfly-junk --confirm
vapourfly junk hide --dry-run
vapourfly recommend --minutes 60 --count 5 [--deck] [--installed-only]
vapourfly playlist import path/to/list.vapourfly-playlist.json
vapourfly playlist export <id> --out list.vapourfly-playlist.json
vapourfly playlist match path/to/list.vapourfly-playlist.json
vapourfly sync collection <playlist-id> --dry-run
vapourfly sync collection <playlist-id> --confirm
vapourfly cache refresh --source igdb|rawg|protondb|pcgw|all
vapourfly backup list
vapourfly backup restore <backup-file>
vapourfly sources status
```

所有写命令默认需要 `--dry-run` 或 `--confirm`，缺二者时报错。

---

## 9. GUI 方案

GUI 依赖 CLI/core 已验证的能力，Phase 5 开始。

页面：

- Library：游戏网格/表格、搜索、过滤。
- Junk：候选、原因、预览 diff、确认写入。
- Recommend：可用时间、Deck mode、推荐结果。
- Playlists：导入、导出、匹配、同步。
- Collections：Steam 集合列表、Vapourfly 集合。
- Data Sources：API key 状态、缓存刷新、失败日志。
- Backups：备份列表、恢复。

GUI 不直接写文件，所有写操作调用 core 的 `WritePlan`。

---

## 10. Cargo 依赖

```toml
[workspace]
members = ["crates/core", "crates/api", "crates/cli", "crates/gui"]
resolver = "2"

[workspace.package]
edition = "2024"
rust-version = "1.88"
license = "MIT OR Apache-2.0"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
anyhow = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = "0.3"
uuid = { version = "1", features = ["v4", "serde"] }
dirs = "6"
walkdir = "2"
sha2 = "0.10"
tempfile = "3"
clap = { version = "4", features = ["derive"] }
ureq = { version = "3", features = ["json"] }
governor = "0.8"
urlencoding = "2"
strsim = "0.11"
keyring = { version = "3", optional = true }
eframe = "0.31"
egui = "0.31"
egui_extras = "0.31"
```

说明：

- `eframe/egui` 使用 0.31，匹配当前实际依赖版本。
- Rust MSRV 设为 1.88，匹配 egui/eframe 生态要求。
- `keyring` 后续用于保存 API secret，MVP 可先使用环境变量。

---

## 11. 测试计划

### 11.1 固定样本测试

| 用例 | 输入 | 期望 |
|---|---|---|
| localconfig apps | `reference/steam-samples/localconfig.vdf` | 解析出 playtime、LastPlayed、Playtime2wks |
| cloud collections | `reference/steam-samples/cloud-storage-namespace-1.json` | 读取 favorite、from-tag、hidden，跳过 is_deleted |
| key 一致性 | upsert 新集合 | outer key 与 entry.key 完全一致 |
| hidden 合并 | hidden 已存在 | AppID 去重、保留旧 version/extra |
| 空 appinfo | appcache 缺失 | 正常降级到 librarycache/appmanifest |
| API key 缺失 | 无 IGDB/RAWG env | scan/recommend 继续运行 |
| 429 | mock API 429 | 退避重试，失败后 stale cache |
| 写入失败 | rename 前抛错 | 目标文件保持原样 |
| 验证失败 | 写入后 JSON 损坏 | 恢复备份 |

### 11.2 CLI 验收

```bash
cargo test --workspace
cargo run -p vapourfly-cli -- doctor --fixtures data/fixtures
cargo run -p vapourfly-cli -- scan --fixtures data/fixtures --format json
cargo run -p vapourfly-cli -- collections list --fixtures data/fixtures
cargo run -p vapourfly-cli -- junk preview --fixtures data/fixtures
cargo run -p vapourfly-cli -- sync collection vapourfly-junk --fixtures data/fixtures --dry-run
```

### 11.3 API Mock 验收

- IGDB token 过期刷新。
- IGDB external_games 按 Steam AppID 解析。
- IGDB game_time_to_beats 秒数映射。
- RAWG 搜索多结果排序。
- ProtonDB 404 → Unknown。
- PCGW redirect → page_name → cargoquery。

---

## 12. 许可证与合规

Vapourfly 代码默认按 `MIT OR Apache-2.0` 准备。

参考项目边界：

| 参考 | 许可证 | 使用方式 |
|---|---|---|
| Depressurizer | GPLv3 | 只阅读行为和格式，不复制实现 |
| TinyWiiBackupManager | GPL-3.0 | 只参考工程分层和 UI 思路，不复制实现 |
| Gameloop.Vdf | MIT | 可参考文本 VDF 行为；Rust 实现独立编写 |
| SteamTools / BD.SteamClient | 以仓库许可证为准 | 只参考路径和格式处理，不复制实现 |
| Steam 样本文件 | 用户本地样本 | 仅用于测试，发布包不包含个人样本 |

规则：

- GPL 源码禁止逐行翻译、复制结构化实现、复制注释。
- Rust VDF/Steam 解析器以公开格式、样本和独立测试驱动实现。
- 发布前生成 `THIRD_PARTY_NOTICES.md`。
- 日志禁止记录 SteamID、账户名、API secret、完整本地路径；debug 模式也要脱敏。

---

## 13. 风险与降级

| 风险 | 降级 |
|---|---|
| Steam 更新 cloudstorage 格式 | 保留 unknown fields，解析失败只读退出 |
| Steam 正在运行导致覆盖 | 默认阻止写入，用户显式允许才继续 |
| IGDB/RAWG key 缺失 | 使用本地数据和已有缓存 |
| IGDB 429 | 限速、退避、stale cache |
| HLTB 页面变化 | 可选模块失败，IGDB time_to_beat 接管 |
| PCGW Cargo schema 变化 | raw JSON 缓存，字段 unknown |
| appinfo.vdf 缺失 | appmanifest + librarycache + Steam Web API |
| Windows registry 读取失败 | 用户手动传 `--steam-dir` |
| 多账户歧义 | CLI 要求 `--account`，GUI 显示选择器 |

---

## 14. 开工顺序

1. 创建 workspace 和 fixtures。
2. 实现 Text VDF parser。
3. 实现路径、账户、libraryfolders。
4. 实现 `localconfig.vdf` apps parser。
5. 实现 cloudstorage collections reader。
6. 实现 `doctor / scan / collections list`。
7. 实现 `WritePlan`、backup、dry-run。
8. 实现 Junk preview/apply/hide。
9. 接入 IGDB token + external_games + game_time_to_beats。
10. 接入 RAWG/ProtonDB/PCGW。
11. 实现 recommend 和 playlist。
12. GUI 接入。

---

## 15. 外部资料

- IGDB API docs: https://api-docs.igdb.com/#getting-started
- RAWG API docs: https://api.rawg.io/docs/
- PCGamingWiki API: https://www.pcgamingwiki.com/wiki/PCGamingWiki:API
- eframe changelog: https://github.com/emilk/egui/blob/master/crates/eframe/CHANGELOG.md

