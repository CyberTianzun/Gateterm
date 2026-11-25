use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::i18n::LanguagePref;
use crate::theme::TerminalPaletteId;

pub const BUILTIN_LOCAL_SERVER_ID: &str = "local-embedded";
pub const LOCAL_SERVER_ID: &str = BUILTIN_LOCAL_SERVER_ID;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    /// HTTP base URL, e.g. `http://127.0.0.1:7683` or `https://term.example.com`
    pub base_url: String,
    pub token: String,
    /// True if this entry points to the built-in embedded server managed by this client
    #[serde(default)]
    pub is_embedded: bool,
    #[serde(default = "default_true")]
    pub auto_connect: bool,
}

fn default_true() -> bool {
    true
}

impl ServerProfile {
    pub fn embedded_default() -> Self {
        Self {
            id: BUILTIN_LOCAL_SERVER_ID.to_string(),
            name: "Local Gateterm".to_string(),
            base_url: format!("http://{}", default_embedded_listen()),
            token: default_embedded_token(),
            is_embedded: true,
            auto_connect: true,
        }
    }

    pub fn ws_url(&self) -> String {
        let trimmed = self.base_url.trim_end_matches('/');
        if let Some(rest) = trimmed.strip_prefix("https://") {
            format!("wss://{rest}/ws")
        } else if let Some(rest) = trimmed.strip_prefix("http://") {
            format!("ws://{rest}/ws")
        } else if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
            if trimmed.ends_with("/ws") {
                trimmed.to_string()
            } else {
                format!("{trimmed}/ws")
            }
        } else {
            format!("ws://{trimmed}/ws")
        }
    }

    pub fn http_base_url(&self) -> String {
        let trimmed = self.base_url.trim_end_matches('/');
        let without_ws = trimmed.strip_suffix("/ws").unwrap_or(trimmed);
        if let Some(rest) = without_ws.strip_prefix("wss://") {
            format!("https://{rest}")
        } else if let Some(rest) = without_ws.strip_prefix("ws://") {
            format!("http://{rest}")
        } else if without_ws.starts_with("http://") || without_ws.starts_with("https://") {
            without_ws.to_string()
        } else {
            format!("http://{without_ws}")
        }
    }

    pub fn access_url(&self) -> String {
        let base = self.http_base_url();
        if self.token.trim().is_empty() {
            base
        } else {
            format!("{base}/#token={}", self.token)
        }
    }
}

fn decode_form_component(raw: &str) -> Result<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                anyhow::ensure!(i + 2 < bytes.len(), "truncated percent-encoding");
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .map_err(|_| anyhow::anyhow!("invalid percent-encoding"))?;
                anyhow::ensure!(
                    hex.bytes().all(|b| b.is_ascii_hexdigit()),
                    "invalid percent-encoding"
                );
                let byte = u8::from_str_radix(hex, 16)?;
                out.push(byte);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| anyhow::anyhow!("invalid UTF-8 in URL parameter"))
}

fn first_param(encoded: &str, target_key: &str) -> Result<Option<String>> {
    for pair in encoded.split('&') {
        let Some((raw_key, raw_val)) = pair.split_once('=') else {
            continue;
        };
        if raw_key != target_key {
            continue;
        }
        let decoded = decode_form_component(raw_val)?;
        if !decoded.is_empty() {
            return Ok(Some(decoded));
        }
    }
    Ok(None)
}

fn param_from_url(parsed: &Url, key: &str) -> Result<Option<String>> {
    if let Some(frag) = parsed.fragment()
        && let Some(val) = first_param(frag, key)?
    {
        return Ok(Some(val));
    }
    if let Some(query) = parsed.query() {
        return first_param(query, key);
    }
    Ok(None)
}

fn normalize_access_title(raw: &str) -> Option<String> {
    let collapsed = raw.replace("\r\n", " ").replace(['\r', '\n'], " ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        return None;
    }
    let capped: String = trimmed.chars().take(128).collect();
    let final_title = capped.trim_end().to_string();
    if final_title.is_empty() {
        None
    } else {
        Some(final_title)
    }
}

/// Parses a Gateterm access URL like `http://127.0.0.1:7683/#token=abc` or `http://host:port?token=abc`
pub fn parse_access_url(raw: &str) -> Result<(String, Option<String>, String)> {
    let input = raw.trim();
    let parsed = if input.contains("://") {
        Url::parse(input)?
    } else {
        Url::parse(&format!("http://{input}"))?
    };

    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL missing host"))?;
    let port_part = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    let scheme = match parsed.scheme() {
        "https" | "wss" => "https",
        _ => "http",
    };
    let base_url = format!("{scheme}://{host}{port_part}");
    let host_display = format!("{host}{port_part}");

    let token = param_from_url(&parsed, "token")?;
    let raw_title = param_from_url(&parsed, "title")?;
    let title = raw_title.as_deref().and_then(normalize_access_title);
    let display_name = title.unwrap_or(host_display);

    Ok((base_url, token, display_name))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopClientConfig {
    #[serde(default)]
    pub language: LanguagePref,
    #[serde(default)]
    pub palette: TerminalPaletteId,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    #[serde(default = "default_scrollback")]
    pub max_scrollback: usize,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    #[serde(default = "default_true")]
    pub start_embedded_server: bool,
    #[serde(default = "default_embedded_listen")]
    pub embedded_listen: String,
    #[serde(default = "default_embedded_token")]
    pub embedded_token: String,
    #[serde(default)]
    pub servers: Vec<ServerProfile>,
}

fn default_font_family() -> String {
    if cfg!(target_os = "windows") {
        "Consolas".to_string()
    } else if cfg!(target_os = "macos") {
        "Menlo".to_string()
    } else {
        "Monospace".to_string()
    }
}

fn default_font_size() -> f32 {
    14.0
}

fn default_line_height() -> f32 {
    1.25
}

fn default_scrollback() -> usize {
    10_000
}

fn default_embedded_listen() -> String {
    "127.0.0.1:17683".to_string()
}

fn default_embedded_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

impl Default for DesktopClientConfig {
    fn default() -> Self {
        let token = default_embedded_token();
        let listen = default_embedded_listen();
        Self {
            language: LanguagePref::Auto,
            palette: TerminalPaletteId::Gate2026Dark,
            font_family: default_font_family(),
            font_size: default_font_size(),
            line_height: default_line_height(),
            max_scrollback: default_scrollback(),
            close_to_tray: true,
            start_embedded_server: true,
            embedded_listen: listen.clone(),
            embedded_token: token.clone(),
            servers: vec![ServerProfile {
                id: BUILTIN_LOCAL_SERVER_ID.to_string(),
                name: "Local Gateterm".to_string(),
                base_url: format!("http://{listen}"),
                token,
                is_embedded: true,
                auto_connect: true,
            }],
        }
    }
}

impl DesktopClientConfig {
    pub fn config_path() -> PathBuf {
        if let Some(proj) = ProjectDirs::from("ai", "eclosion", "gateterm") {
            proj.config_dir().join("desktop-client.toml")
        } else {
            PathBuf::from("desktop-client.toml")
        }
    }

    pub fn load() -> Self {
        Self::load_from(&Self::config_path()).unwrap_or_default()
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)?;
        let mut cfg: Self = toml::from_str(&raw)?;
        cfg.normalize();
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        fs::write(path, raw)?;
        Ok(())
    }

    pub fn normalize(&mut self) {
        self.font_size = self.font_size.clamp(9.0, 32.0);
        self.line_height = self.line_height.clamp(1.0, 2.2);
        self.max_scrollback = self.max_scrollback.clamp(500, 200_000);
        if self.embedded_token.trim().is_empty() {
            self.embedded_token = default_embedded_token();
        }
        if self.embedded_listen.trim().is_empty() {
            self.embedded_listen = default_embedded_listen();
        }
        if !self.servers.iter().any(|s| s.is_embedded) {
            let connect_addr = client_connect_host_port(&self.embedded_listen);
            self.servers.insert(
                0,
                ServerProfile {
                    id: BUILTIN_LOCAL_SERVER_ID.to_string(),
                    name: "Local Gateterm".to_string(),
                    base_url: format!("http://{connect_addr}"),
                    token: self.embedded_token.clone(),
                    is_embedded: true,
                    auto_connect: self.start_embedded_server,
                },
            );
        } else if let Some(local) = self.servers.iter_mut().find(|s| s.is_embedded) {
            let connect_addr = client_connect_host_port(&self.embedded_listen);
            local.base_url = format!("http://{connect_addr}");
            local.token = self.embedded_token.clone();
        }
    }

    pub fn upsert_server(&mut self, profile: ServerProfile) {
        if let Some(existing) = self.servers.iter_mut().find(|s| s.id == profile.id) {
            *existing = profile;
        } else {
            self.servers.push(profile);
        }
    }

    pub fn remove_server(&mut self, id: &str) {
        self.servers.retain(|s| s.id != id || s.is_embedded);
    }
}

pub fn generate_random_token() -> String {
    default_embedded_token()
}

pub fn split_listen_host_port(listen: &str) -> (String, u16) {
    if let Ok(addr) = listen.trim().parse::<std::net::SocketAddr>() {
        let host = if addr.ip().is_ipv6() {
            format!("[{}]", addr.ip())
        } else {
            addr.ip().to_string()
        };
        (host, addr.port())
    } else if let Some((h, p)) = listen.trim().rsplit_once(':') {
        let port = p.parse::<u16>().unwrap_or(0);
        let host = if h.trim().is_empty() {
            "127.0.0.1".to_string()
        } else {
            h.trim().to_string()
        };
        (host, port)
    } else {
        ("127.0.0.1".to_string(), 17683)
    }
}

pub fn client_connect_host_port(listen: &str) -> String {
    if let Ok(addr) = listen.trim().parse::<std::net::SocketAddr>() {
        let host = if addr.ip().is_unspecified() {
            "127.0.0.1".to_string()
        } else if addr.ip().is_ipv6() {
            format!("[{}]", addr.ip())
        } else {
            addr.ip().to_string()
        };
        format!("{host}:{}", addr.port())
    } else {
        listen.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jupyter_style_access_url() {
        let (base, token, name) =
            parse_access_url("http://192.168.1.20:7683/#token=my-secret-token").unwrap();
        assert_eq!(base, "http://192.168.1.20:7683");
        assert_eq!(token.as_deref(), Some("my-secret-token"));
        assert_eq!(name, "192.168.1.20:7683");
    }

    #[test]
    fn parses_fragment_token_and_title() {
        let (base, token, name) = parse_access_url(
            "http://192.168.1.20:7683/#token=a%2Bb+%26%3D%23%E5%AF%86%E7%A0%81&title=%E7%A0%94%E5%8F%91+1%E5%8F%B7",
        )
        .unwrap();
        assert_eq!(base, "http://192.168.1.20:7683");
        assert_eq!(token.as_deref(), Some("a+b &=#密码"));
        assert_eq!(name, "研发 1号");
    }

    #[test]
    fn parses_query_token_and_title_fallback() {
        let (base, token, name) =
            parse_access_url("http://192.168.1.20:7683/?token=qtok&title=Query+Box").unwrap();
        assert_eq!(base, "http://192.168.1.20:7683");
        assert_eq!(token.as_deref(), Some("qtok"));
        assert_eq!(name, "Query Box");
    }

    #[test]
    fn fragment_wins_over_query_when_both_exist() {
        let (_, token, name) = parse_access_url(
            "http://192.168.1.20:7683/?token=qtok&title=Query+Box#token=ftok&title=Frag+Box",
        )
        .unwrap();
        assert_eq!(token.as_deref(), Some("ftok"));
        assert_eq!(name, "Frag Box");
    }

    #[test]
    fn blank_or_newline_title_falls_back_to_host_port() {
        let (_, token, name) =
            parse_access_url("http://192.168.1.20:7683/#token=tok&title=%20%0A").unwrap();
        assert_eq!(token.as_deref(), Some("tok"));
        assert_eq!(name, "192.168.1.20:7683");
    }

    #[test]
    fn truncates_title_to_128_unicode_scalars() {
        let long_title = "🚀".repeat(130);
        let fragment = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("token", "tok")
            .append_pair("title", &long_title)
            .finish();
        let raw_url = format!("http://192.168.1.20:7683/#{fragment}");
        let (_, token, name) = parse_access_url(&raw_url).unwrap();
        assert_eq!(token.as_deref(), Some("tok"));
        assert_eq!(name.chars().count(), 128);
        assert_eq!(name, "🚀".repeat(128));
    }

    #[test]
    fn rejects_malformed_percent_escapes_and_invalid_utf8() {
        assert!(parse_access_url("http://192.168.1.20:7683/#token=%ZZ").is_err());
        assert!(parse_access_url("http://192.168.1.20:7683/#token=ok&title=%ZZ").is_err());
        assert!(parse_access_url("http://192.168.1.20:7683/#token=ok&title=abc%2").is_err());
        assert!(parse_access_url("http://192.168.1.20:7683/#token=ok&title=%FF").is_err());
        assert!(parse_access_url("http://192.168.1.20:7683/?token=ok&title=%ZZ").is_err());
    }

    #[test]
    fn server_profile_access_url_stays_token_only() {
        let p = ServerProfile {
            id: "s1".into(),
            name: "Local Gateterm".into(),
            base_url: "http://127.0.0.1:7683".into(),
            token: "tok123".into(),
            is_embedded: false,
            auto_connect: true,
        };
        assert_eq!(p.access_url(), "http://127.0.0.1:7683/#token=tok123");
    }

    #[test]
    fn server_profile_converts_http_and_ws_urls() {
        let p = ServerProfile {
            id: "s1".into(),
            name: "Dev".into(),
            base_url: "https://term.example.com:8443/".into(),
            token: "tok".into(),
            is_embedded: false,
            auto_connect: true,
        };
        assert_eq!(p.ws_url(), "wss://term.example.com:8443/ws");
        assert_eq!(p.http_base_url(), "https://term.example.com:8443");
    }

    #[test]
    fn config_roundtrip_preserves_embedded_server() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("client.toml");
        let mut cfg = DesktopClientConfig::default();
        cfg.font_size = 16.0;
        cfg.save_to(&path).unwrap();
        let loaded = DesktopClientConfig::load_from(&path).unwrap();
        assert_eq!(loaded.font_size, 16.0);
        assert!(loaded.servers.iter().any(|s| s.is_embedded));
    }
}
