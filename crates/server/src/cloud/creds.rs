//! `cloud.json` 的共享只读视图。握手(回带 cloud_server_id)与通知器共用,
//! 按 (mtime, 文件长度) 缓存:desktop 登录/退出后不用重启 server 就能生效;
//! 单看 mtime 在粗粒度文件系统上不够 —— 同一个 tick 里内容换了,mtime 可能没变。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use octoterm_cloud_client::credentials::{self, Credentials};

type Cached = Option<((SystemTime, u64), Credentials)>;

#[derive(Clone, Default)]
pub struct SharedCreds {
    inner: Option<Arc<Inner>>,
}

struct Inner {
    path: PathBuf,
    cached: Mutex<Cached>,
}

impl SharedCreds {
    /// `None` 表示没有可用的配置目录:永远视为未登录。
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { inner: path.map(|path| Arc::new(Inner { path, cached: Mutex::new(None) })) }
    }

    pub fn path(&self) -> Option<&Path> {
        self.inner.as_ref().map(|i| i.path.as_path())
    }

    pub fn get(&self) -> Option<Credentials> {
        let inner = self.inner.as_ref()?;
        let mut cached = inner.cached.lock().unwrap();
        let Ok(meta) = std::fs::metadata(&inner.path) else {
            *cached = None;
            return None;
        };
        let Ok(mtime) = meta.modified() else {
            *cached = None;
            return None;
        };
        let key = (mtime, meta.len());
        if let Some((k, c)) = cached.as_ref()
            && *k == key
        {
            return Some(c.clone());
        }
        match credentials::load(&inner.path) {
            Ok(Some(c)) => {
                *cached = Some((key, c.clone()));
                Some(c)
            }
            Ok(None) => {
                *cached = None;
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "cloud.json 读取失败,视为未登录");
                None
            }
        }
    }

    pub fn server_id(&self) -> Option<String> {
        self.get().and_then(|c| c.server_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 粗粒度文件系统(常见于 HFS+/一些容器挂载)上 mtime 的精度可能只有 1 秒;
    /// 同一秒内两次写入内容不同但 mtime 相同的 cloud.json,缓存键只看 mtime 会读到旧内容。
    #[test]
    fn cred_cache_busts_on_length_change_even_when_mtime_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        let creds1 = Credentials { url: "https://cloud.example.com".into(), email: "a@example.com".into(), token: "t1".into(), server_id: None };
        credentials::save(&path, &creds1).unwrap();
        let mtime1 = std::fs::metadata(&path).unwrap().modified().unwrap();
        let shared = SharedCreds::new(Some(path.clone()));
        assert_eq!(shared.get(), Some(creds1));

        let creds2 = Credentials {
            url: "https://cloud.example.com".into(),
            email: "someone-else@example.com".into(),
            token: "a-much-longer-replacement-token".into(),
            server_id: Some("srv-2".into()),
        };
        credentials::save(&path, &creds2).unwrap();
        // 把新内容的 mtime 强行拨回旧值,模拟低精度文件系统下两次写撞在同一个 tick 里。
        std::fs::File::open(&path).unwrap().set_modified(mtime1).unwrap();

        assert_eq!(shared.get(), Some(creds2), "内容变了但 mtime 没变时,缓存也该失效");
    }

    #[test]
    fn server_id_none_when_logged_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        let shared = SharedCreds::new(Some(path.clone()));
        assert_eq!(shared.server_id(), None);

        let creds = Credentials { url: "https://cloud.example.com".into(), email: "a@example.com".into(), token: "t".into(), server_id: Some("s".into()) };
        credentials::save(&path, &creds).unwrap();
        assert_eq!(shared.server_id(), Some("s".into()));
    }
}
