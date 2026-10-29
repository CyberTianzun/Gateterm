//! Start-at-login registration (macOS LaunchAgent plist / Windows HKCU Run registry key),
//! aligned with `clients/gateterm-desktop-web/src/autostart.rs`.

use anyhow::{Context, Result};

pub const LABEL: &str = "com.gateterm.desktop";

#[cfg(any(target_os = "macos", windows))]
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

#[cfg(not(any(target_os = "macos", windows)))]
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
