//! `cloud.json`:Cloud 登录态。文件存在 = 已登录。
//!
//! 里面的 token 能以这台机器的身份调用 Cloud,所以 Unix 上权限是 0600,
//! 并且先写临时文件再 rename,避免读到写了一半的文件。

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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

pub fn save(path: &Path, creds: &Credentials) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    // 先删:OpenOptions 的 mode 只在创建时生效,残留的临时文件可能是别的权限。
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
}
