//! `cloud.json`:Cloud 登录态。文件存在 = 已登录。
//!
//! 里面的 token 能以这台机器的身份调用 Cloud,所以 Unix 上权限是 0600,
//! 并且先写临时文件再 rename,避免读到写了一半的文件。

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub url: String,
    pub email: String,
    pub token: String,
    #[serde(default)]
    pub server_id: Option<String>,
}

/// 与 config.toml、hook-token 同目录。`OCTOTERM_CLOUD_FILE` 可覆盖(测试与冒烟用)。
pub fn default_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OCTOTERM_CLOUD_FILE").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    directories::ProjectDirs::from("", "", "octoterm").map(|d| d.config_dir().join("cloud.json"))
}

pub fn load(path: &Path) -> io::Result<Option<Credentials>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// 同进程内给临时文件名去重;配合 pid 后就不会和另一个 CLI/desktop/notifier 进程撞上。
static SAVE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn save(path: &Path, creds: &Credentials) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // 每次调用独占一个临时文件名:CLI、desktop、notifier 可能同时往同一个 cloud.json
    // 写,共享一个固定的 `.tmp` 名会让它们互相踩到对方还没 rename 完的临时文件。
    let n = SAVE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.{}.{n}.tmp", std::process::id()));
    // 先删:这个名字本该是独占的,但万一是上次崩溃留下的残留,mode 只在创建时生效。
    let _ = fs::remove_file(&tmp);
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(creds)?)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

pub fn delete(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Credentials {
        Credentials {
            url: "https://cloud.example.com".into(),
            email: "a@example.com".into(),
            token: "oct_abc".into(),
            server_id: Some("s1".into()),
        }
    }

    #[test]
    fn roundtrip_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/cloud.json");
        assert_eq!(load(&path).unwrap(), None);
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path).unwrap(), Some(sample()));
        delete(&path).unwrap();
        assert_eq!(load(&path).unwrap(), None);
        delete(&path).unwrap(); // 不存在也不报错
    }

    #[cfg(unix)]
    #[test]
    fn file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        save(&path, &sample()).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn corrupt_file_is_an_error_not_logged_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        fs::write(&path, b"{not json").unwrap();
        assert!(load(&path).is_err());
    }

    /// CLI、desktop、notifier 都可能同时往同一个 cloud.json 写:并发 save() 不该互相
    /// 踩到对方的临时文件(表现为莫名其妙的 AlreadyExists / NotFound)。
    #[test]
    fn concurrent_saves_do_not_clobber_each_others_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.json");
        let results: Vec<io::Result<()>> = std::thread::scope(|scope| {
            (0..16)
                .map(|i| {
                    let path = &path;
                    scope.spawn(move || {
                        let creds = Credentials {
                            url: "https://cloud.example.com".into(),
                            email: format!("user{i}@example.com"),
                            token: format!("oct_token_{i}"),
                            server_id: None,
                        };
                        save(path, &creds)
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect()
        });
        for r in &results {
            assert!(r.is_ok(), "concurrent save() failed: {r:?}");
        }
        assert!(load(&path).unwrap().is_some());
    }
}
