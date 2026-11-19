//! Start-at-login registration (macOS LaunchAgent plist / Windows HKCU Run registry key /
//! Linux XDG Autostart desktop entry), aligned with `clients/desktop-web/src/autostart.rs`.

use anyhow::{Context, Result};

pub const LABEL: &str = "com.gateterm.desktop";

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
fn exe_path() -> Result<String> {
    Ok(std::env::current_exe()
        .context("Could not determine current executable path")?
        .to_string_lossy()
        .into_owned())
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::path::{Path, PathBuf};

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    pub fn plist_xml(exe: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
"#,
            label = LABEL,
            exe = xml_escape(exe),
        )
    }

    fn plist_path() -> Result<PathBuf> {
        let home = directories::BaseDirs::new().context("Could not locate HOME")?;
        Ok(home
            .home_dir()
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist")))
    }

    fn registered_target_at(path: &Path) -> Result<Option<String>> {
        if !path.exists() {
            return Ok(None);
        }
        let value = plist::Value::from_file(path)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(value
            .as_dictionary()
            .and_then(|d| d.get("ProgramArguments"))
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_string())
            .map(str::to_string))
    }

    pub fn is_enabled() -> Result<bool> {
        let Some(target) = registered_target_at(&plist_path()?)? else {
            return Ok(false);
        };
        Ok(Path::new(&target).exists())
    }

    pub fn set(enabled: bool) -> Result<()> {
        let path = plist_path()?;
        if enabled {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, plist_xml(&exe_path()?))
                .with_context(|| format!("Failed to write {}", path.display()))?;
        } else if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::path::Path;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "gateterm";

    fn strip_quotes(s: &str) -> &str {
        s.strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(s)
    }

    fn registered_target() -> Result<Option<String>> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(run) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ) else {
            return Ok(None);
        };
        Ok(run
            .get_value::<String, _>(VALUE)
            .ok()
            .map(|v| strip_quotes(&v).to_string()))
    }

    pub fn is_enabled() -> Result<bool> {
        let Some(target) = registered_target()? else {
            return Ok(false);
        };
        Ok(Path::new(&target).exists())
    }

    pub fn set(enabled: bool) -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (run, _) = hkcu
            .create_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE)
            .context("Failed to open HKCU Run registry key")?;
        if enabled {
            run.set_value(VALUE, &format!("\"{}\"", exe_path()?))
                .context("Failed to write startup registry value")?;
        } else {
            match run.delete_value(VALUE) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).context("Failed to delete startup registry value"),
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn strips_matching_quotes() {
            assert_eq!(
                strip_quotes(r#""C:\path\to\exe.exe""#),
                r"C:\path\to\exe.exe"
            );
            assert_eq!(strip_quotes(r"C:\path\to\exe.exe"), r"C:\path\to\exe.exe");
            assert_eq!(strip_quotes(r#""C:\odd"#), r#""C:\odd"#);
        }
    }
}

/// Linux: XDG Autostart `.desktop` entry under `$XDG_CONFIG_HOME/autostart`, honoured by GNOME,
/// KDE and uwsm / systemd-xdg-autostart-generator based sessions such as Omarchy (Hyprland).
#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::path::{Path, PathBuf};

    const FILE_NAME: &str = "gateterm-desktop-native.desktop";

    /// Desktop Entry `Exec` quoting: wrap in double quotes, backslash-escape `"` `` ` `` `$` `\`,
    /// and double `%` (field-code prefix).
    fn exec_quote(exe: &str) -> String {
        let mut out = String::with_capacity(exe.len() + 2);
        out.push('"');
        for c in exe.chars() {
            match c {
                '"' | '`' | '$' | '\\' => {
                    out.push('\\');
                    out.push(c);
                }
                '%' => out.push_str("%%"),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }

    fn exec_unquote(exec: &str) -> String {
        let exec = exec.trim();
        let Some(rest) = exec.strip_prefix('"') else {
            return exec
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .replace("%%", "%");
        };
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.extend(chars.next()),
                '"' => break,
                _ => out.push(c),
            }
        }
        out.replace("%%", "%")
    }

    pub fn desktop_entry(exe: &str) -> String {
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Gateterm\n\
             Comment=Gateterm desktop terminal\n\
             Exec={exec}\n\
             Icon=gateterm\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n\
             X-Gateterm-Label={label}\n",
            exec = exec_quote(exe),
            label = LABEL,
        )
    }

    fn entry_path() -> Result<PathBuf> {
        let base = directories::BaseDirs::new().context("Could not locate HOME")?;
        Ok(base.config_dir().join("autostart").join(FILE_NAME))
    }

    fn registered_target_at(path: &Path) -> Result<Option<String>> {
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if text.lines().any(|l| l.trim() == "Hidden=true") {
            return Ok(None);
        }
        Ok(text
            .lines()
            .find_map(|l| l.trim().strip_prefix("Exec="))
            .map(exec_unquote)
            .filter(|s| !s.is_empty()))
    }

    pub fn is_enabled() -> Result<bool> {
        let Some(target) = registered_target_at(&entry_path()?)? else {
            return Ok(false);
        };
        Ok(Path::new(&target).exists())
    }

    pub fn set(enabled: bool) -> Result<()> {
        let path = entry_path()?;
        if enabled {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, desktop_entry(&exe_path()?))
                .with_context(|| format!("Failed to write {}", path.display()))?;
        } else if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn round_trips_paths_through_the_desktop_entry() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(FILE_NAME);
            for exe in [
                "/home/me/.local/bin/gateterm-desktop-native",
                r#"/opt/my apps/50% "gt"/$x\gateterm"#,
            ] {
                std::fs::write(&path, desktop_entry(exe)).unwrap();
                assert_eq!(registered_target_at(&path).unwrap().as_deref(), Some(exe));
            }
        }

        #[test]
        fn a_missing_or_hidden_entry_is_not_registered() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(FILE_NAME);
            assert_eq!(registered_target_at(&path).unwrap(), None);
            std::fs::write(&path, desktop_entry("/usr/bin/gateterm") + "Hidden=true\n").unwrap();
            assert_eq!(registered_target_at(&path).unwrap(), None);
        }
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod imp {
    use anyhow::Result;
    pub fn is_enabled() -> Result<bool> {
        Ok(false)
    }
    pub fn set(_enabled: bool) -> Result<()> {
        Ok(())
    }
}

pub use imp::{is_enabled, set};
