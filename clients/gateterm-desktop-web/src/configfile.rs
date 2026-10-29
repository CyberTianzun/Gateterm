//! config.toml 的写入侧。
//!
//! server 自己永远不写这个文件(见 `gateterm_server::config::Config::load`),
//! 写是 desktop 的职责。用 toml_edit 而不是 serde 序列化整个 Config:配置文件
//! 是鼓励用户手写的,把注释、顺序、空行碾掉换不来任何好处。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::{value, DocumentMut};
use crate::i18n::LanguagePref;

/// desktop 允许改的字段。其余键(window_size、[[launcher]] 等)原样保留。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editable {
    pub listen: SocketAddr,
    /// `None` 表示不固定 token —— 移除该键,server 下次启动随机生成。
    pub token: Option<String>,
    pub language: LanguagePref,
}

/// 与 server 用同一个平台配置目录(`gateterm_server::config` 里的私有 `default_path`
/// 的等价实现),两边必须指向同一个文件。
pub fn default_path() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "gateterm")
        .context("无法确定配置目录")?;
    Ok(dirs.config_dir().join("config.toml"))
}

/// Missing, unreadable or unsupported preferences follow the OS. Config parse
/// errors are reported separately by the existing startup error banner.
pub fn load_language(path: &Path) -> LanguagePref {
    std::fs::read_to_string(path).ok()
        .and_then(|text| text.parse::<DocumentMut>().ok())
        .and_then(|doc| doc.get("desktop")?.get("language")?.as_str().map(LanguagePref::from_config))
        .unwrap_or_default()
}

pub fn language() -> LanguagePref {
    default_path().map(|path| load_language(&path)).unwrap_or_default()
}

/// 就地写回。文件不存在时连同父目录一起创建。
pub fn save(path: &Path, edit: &Editable) -> Result<()> {
    // 只有「文件不存在」才当空文档处理;权限不足、路径其实是目录等其他
    // 读取失败一律向上传播 —— 否则会被 write_atomic 静默 rename 成空文件,
    // 把用户手写的注释、[[launcher]] 段全部清掉。
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("无法读取 {}", path.display())),
    };
    let mut doc: DocumentMut = existing
        .parse()
        .with_context(|| format!("{} 解析失败", path.display()))?;

    doc["listen"] = value(edit.listen.to_string());
    match &edit.token {
        Some(t) => doc["token"] = value(t.as_str()),
        None => {
            doc.remove("token");
        }
    }

    let desktop = doc.entry("desktop").or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let desktop = desktop.as_table_like_mut().context("[desktop] must be a table")?;
    desktop.insert("language", value(edit.language.code()));

    write_atomic(path, doc.to_string().as_bytes())
}

/// 先写同目录的 .tmp 再 rename:写到一半失败不会留下半个配置文件。
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("无法创建目录 {}", dir.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("无法写入 {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("无法替换 {}", path.display()))?;
    Ok(())
}
