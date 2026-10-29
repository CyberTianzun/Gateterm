//! Reads and updates `gateterm`'s shared `config.toml` (preserving user comments via `toml_edit`),
//! aligned with `clients/gateterm-desktop-web/src/configfile.rs`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use gateterm_server::config::Config as ServerConfig;
use toml_edit::{DocumentMut, value};

use crate::i18n::LanguagePref;

#[derive(Debug, Clone)]
pub struct ServerConfigSummary {
    pub window_size: String,
    pub launchers: Vec<(String, String)>,
    pub config_error: Option<String>,
}

impl Default for ServerConfigSummary {
    fn default() -> Self {
        Self {
            window_size: "smallest".to_string(),
            launchers: Vec::new(),
            config_error: None,
        }
    }
}

pub fn default_path() -> Result<PathBuf> {
    let dirs =
        directories::ProjectDirs::from("", "", "gateterm").context("Cannot locate config dir")?;
    Ok(dirs.config_dir().join("config.toml"))
}

pub fn load_server_summary() -> ServerConfigSummary {
    match ServerConfig::load(None) {
        Ok(cfg) => ServerConfigSummary {
            window_size: format!("{:?}", cfg.window_size).to_lowercase(),
            launchers: cfg
                .launchers
                .iter()
                .map(|l| (l.name.clone(), l.command.join(" ")))
                .collect(),
            config_error: None,
        },
        Err(e) => ServerConfigSummary {
            window_size: "smallest".to_string(),
            launchers: Vec::new(),
            config_error: Some(format!("{e:#}")),
        },
    }
}

pub fn save_shared_server_config(
    path: &Path,
    listen: SocketAddr,
    token: Option<&str>,
    language: LanguagePref,
) -> Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("Could not read {}", path.display())),
    };
    let mut doc: DocumentMut = existing
        .parse()
        .with_context(|| format!("Failed to parse {}", path.display()))?;

    doc["listen"] = value(listen.to_string());
    match token.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => doc["token"] = value(t),
        None => {
            doc.remove("token");
        }
    }

    let lang_code = match language {
        LanguagePref::Auto => "auto",
        LanguagePref::ZhCn => "zh-CN",
        LanguagePref::En => "en",
    };
    let desktop = doc
        .entry("desktop")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let desktop = desktop
        .as_table_like_mut()
        .context("[desktop] must be a table")?;
    desktop.insert("language", value(lang_code));

    write_atomic(path, doc.to_string().as_bytes())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create directory {}", dir.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("Failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("Failed to replace {}", path.display()))?;
    Ok(())
}
