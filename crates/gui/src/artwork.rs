//! Steam artwork for library capsules, hero banners and logos.
//!
//! Steam already keeps library art on disk under `appcache/librarycache`,
//! so the common case needs no network at all. Anything missing there is
//! fetched once from the Steam CDN on a small background pool and kept under
//! Vapourfly's cache directory. A miss is remembered with an empty marker
//! file so it is not retried every launch.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One piece of Steam library art.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ArtKind {
    /// 460×215 landscape capsule; every app has one.
    Header,
    /// 600×900 portrait library capsule.
    Portrait,
    /// 3840×1240 library hero banner.
    Hero,
    /// Transparent title logo drawn over the hero.
    Logo,
}

impl ArtKind {
    /// File name Steam uses for this art, both in the CDN path and inside
    /// the per-app library cache folder.
    fn file_name(self) -> &'static str {
        match self {
            Self::Header => "header.jpg",
            Self::Portrait => "library_600x900.jpg",
            Self::Hero => "library_hero.jpg",
            Self::Logo => "logo.png",
        }
    }

    fn cdn_uri(self, app_id: u32) -> String {
        format!(
            "https://cdn.cloudflare.steamstatic.com/steam/apps/{app_id}/{}",
            self.file_name()
        )
    }
}

type Key = (u32, ArtKind);

/// Downloads that completed since the last lookup: key and saved file.
type Finished = Arc<Mutex<Vec<(Key, Option<PathBuf>)>>>;

const WORKERS: usize = 4;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

type Wake = Arc<dyn Fn() + Send + Sync>;

struct Inner {
    steam_dir: Option<PathBuf>,
    network: bool,
    resolved: HashMap<Key, Option<PathBuf>>,
    inflight: HashSet<Key>,
    queue: Option<Sender<Key>>,
}

pub(crate) struct ArtworkStore {
    cache_dir: PathBuf,
    inner: RefCell<Inner>,
    finished: Finished,
    wake: Wake,
}

impl ArtworkStore {
    pub(crate) fn new(cache_dir: PathBuf, wake: Wake) -> Self {
        Self {
            cache_dir: cache_dir.join("artwork"),
            inner: RefCell::new(Inner {
                steam_dir: None,
                network: false,
                resolved: HashMap::new(),
                inflight: HashSet::new(),
                queue: None,
            }),
            finished: Arc::new(Mutex::new(Vec::new())),
            wake,
        }
    }

    /// Point the store at the scanned Steam install and allow or forbid
    /// downloads. Changing either forgets earlier misses so they are retried.
    pub(crate) fn configure(&self, steam_dir: Option<PathBuf>, network: bool) {
        let mut inner = self.inner.borrow_mut();
        if inner.steam_dir != steam_dir || inner.network != network {
            inner.steam_dir = steam_dir;
            inner.network = network;
            inner.resolved.retain(|_, path| path.is_some());
        }
    }

    pub(crate) fn header(&self, app_id: u32) -> Option<PathBuf> {
        self.get(app_id, ArtKind::Header)
    }

    pub(crate) fn portrait(&self, app_id: u32) -> Option<PathBuf> {
        self.get(app_id, ArtKind::Portrait)
    }

    pub(crate) fn hero(&self, app_id: u32) -> Option<PathBuf> {
        self.get(app_id, ArtKind::Hero)
    }

    pub(crate) fn logo(&self, app_id: u32) -> Option<PathBuf> {
        self.get(app_id, ArtKind::Logo)
    }

    /// Local path of `kind` art for `app_id`, if one is available now.
    /// Returns `None` while a download is pending or when no art exists.
    fn get(&self, app_id: u32, kind: ArtKind) -> Option<PathBuf> {
        self.drain_finished();
        let key = (app_id, kind);
        let mut inner = self.inner.borrow_mut();
        if let Some(found) = inner.resolved.get(&key) {
            return found.clone();
        }
        if inner.inflight.contains(&key) {
            return None;
        }

        if let Some(path) = local_steam_art(inner.steam_dir.as_deref(), app_id, kind) {
            inner.resolved.insert(key, Some(path.clone()));
            return Some(path);
        }
        let cached = cached_path(&self.cache_dir, key);
        if cached.is_file() {
            inner.resolved.insert(key, Some(cached.clone()));
            return Some(cached);
        }
        if miss_marker(&self.cache_dir, key).is_file() || !inner.network {
            inner.resolved.insert(key, None);
            return None;
        }

        let queue = inner
            .queue
            .get_or_insert_with(|| {
                spawn_workers(
                    self.cache_dir.clone(),
                    Arc::clone(&self.finished),
                    Arc::clone(&self.wake),
                )
            })
            .clone();
        if queue.send(key).is_ok() {
            inner.inflight.insert(key);
        } else {
            inner.resolved.insert(key, None);
        }
        None
    }

    fn drain_finished(&self) {
        let done = std::mem::take(&mut *self.finished.lock().expect("artwork results"));
        if done.is_empty() {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        for (key, path) in done {
            inner.inflight.remove(&key);
            inner.resolved.insert(key, path);
        }
    }
}

fn cached_path(cache_dir: &Path, (app_id, kind): Key) -> PathBuf {
    cache_dir.join(format!("{app_id}_{}", kind.file_name()))
}

fn miss_marker(cache_dir: &Path, key: Key) -> PathBuf {
    cached_path(cache_dir, key).with_extension("none")
}

/// Steam has used two layouts for the library cache: flat
/// `<appid>_<file>` names, and per-app folders that may nest the image one
/// level deeper under a content hash.
fn local_steam_art(steam_dir: Option<&Path>, app_id: u32, kind: ArtKind) -> Option<PathBuf> {
    let cache = steam_dir?.join("appcache").join("librarycache");
    let file = kind.file_name();
    let flat = cache.join(format!("{app_id}_{file}"));
    if flat.is_file() {
        return Some(flat);
    }
    let folder = cache.join(app_id.to_string());
    let direct = folder.join(file);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(&folder)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(file))
        .find(|candidate| candidate.is_file())
}

fn spawn_workers(cache_dir: PathBuf, finished: Finished, wake: Wake) -> Sender<Key> {
    let (tx, rx) = mpsc::channel::<Key>();
    let rx = Arc::new(Mutex::new(rx));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .user_agent(format!("Vapourfly/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    for _ in 0..WORKERS {
        let rx: Arc<Mutex<Receiver<Key>>> = Arc::clone(&rx);
        let agent = agent.clone();
        let cache_dir = cache_dir.clone();
        let finished = Arc::clone(&finished);
        let wake = Arc::clone(&wake);
        std::thread::spawn(move || {
            loop {
                let next = rx.lock().expect("artwork queue").recv();
                let Ok(key) = next else { break };
                let path = download(&agent, &cache_dir, key);
                finished.lock().expect("artwork results").push((key, path));
                wake();
            }
        });
    }
    tx
}

fn download(agent: &ureq::Agent, cache_dir: &Path, key: Key) -> Option<PathBuf> {
    std::fs::create_dir_all(cache_dir).ok()?;
    let target = cached_path(cache_dir, key);
    let Ok(mut response) = agent.get(&key.1.cdn_uri(key.0)).call() else {
        // Network failure: leave no marker so a later launch retries.
        return None;
    };
    if response.status() != 200 {
        let _ = std::fs::write(miss_marker(cache_dir, key), b"");
        return None;
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_IMAGE_BYTES)
        .read_to_vec()
        .ok()?;
    let partial = target.with_extension("part");
    std::fs::write(&partial, bytes).ok()?;
    std::fs::rename(&partial, &target).ok()?;
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_art_supports_flat_and_nested_layouts() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("appcache/librarycache");
        std::fs::create_dir_all(cache.join("20/abc123")).unwrap();
        std::fs::write(cache.join("10_header.jpg"), b"x").unwrap();
        std::fs::write(cache.join("10_library_600x900.jpg"), b"x").unwrap();
        std::fs::write(cache.join("20/abc123/header.jpg"), b"x").unwrap();
        std::fs::write(cache.join("20/library_hero.jpg"), b"x").unwrap();

        let steam = Some(dir.path());
        assert_eq!(
            local_steam_art(steam, 10, ArtKind::Header),
            Some(cache.join("10_header.jpg"))
        );
        assert_eq!(
            local_steam_art(steam, 10, ArtKind::Portrait),
            Some(cache.join("10_library_600x900.jpg"))
        );
        assert_eq!(
            local_steam_art(steam, 20, ArtKind::Header),
            Some(cache.join("20/abc123/header.jpg"))
        );
        assert_eq!(
            local_steam_art(steam, 20, ArtKind::Hero),
            Some(cache.join("20/library_hero.jpg"))
        );
        assert_eq!(local_steam_art(steam, 20, ArtKind::Logo), None);
        assert_eq!(local_steam_art(steam, 30, ArtKind::Header), None);
    }

    #[test]
    fn cdn_uris_follow_steam_library_paths() {
        assert_eq!(
            ArtKind::Portrait.cdn_uri(730),
            "https://cdn.cloudflare.steamstatic.com/steam/apps/730/library_600x900.jpg"
        );
        assert_eq!(
            ArtKind::Logo.cdn_uri(730),
            "https://cdn.cloudflare.steamstatic.com/steam/apps/730/logo.png"
        );
    }

    #[test]
    fn miss_markers_are_per_kind() {
        let dir = Path::new("/cache");
        assert_eq!(
            miss_marker(dir, (7, ArtKind::Hero)),
            dir.join("7_library_hero.none")
        );
        assert_ne!(
            miss_marker(dir, (7, ArtKind::Hero)),
            miss_marker(dir, (7, ArtKind::Header))
        );
    }

    #[test]
    fn offline_store_never_queues_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtworkStore::new(dir.path().to_path_buf(), Arc::new(|| {}));
        store.configure(None, false);
        assert_eq!(store.header(42), None);
        assert_eq!(store.portrait(42), None);
        assert!(store.inner.borrow().queue.is_none());
        assert!(store.inner.borrow().inflight.is_empty());
    }
}
