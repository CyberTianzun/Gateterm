//! 开机自启。这一项不进 config.toml —— 它是 desktop 自己的行为,不是 server 的配置。
//!
//! macOS 用 LaunchAgent plist(放进 ~/Library/LaunchAgents 就会在下次登录时生效,
//! 不需要 launchctl),Windows 用 HKCU 的 Run 键,Linux 用 XDG Autostart 的 .desktop 文件。

use anyhow::{Context, Result};

pub const LABEL: &str = "com.gateterm.desktop";

fn exe_path() -> Result<String> {
    Ok(std::env::current_exe()
        .context("无法取得自身路径")?
        .to_string_lossy()
        .into_owned())
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::path::{Path, PathBuf};

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
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
        let home = directories::BaseDirs::new().context("无法确定 HOME")?;
        Ok(home.home_dir().join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
    }

    /// 解析某个 plist 文件里 `ProgramArguments` 数组的第一项(即注册的可执行文件路径)。
    /// 文件不存在时是 `Ok(None)`(没有注册,不是错误);文件存在但不是合法的
    /// LaunchAgent plist 才是 `Err`。
    ///
    /// 接受路径参数而不是自己去找 HOME,这样单测能喂一个临时文件进来,不用碰
    /// 真实的 `~/Library/LaunchAgents`。
    fn registered_target_at(path: &Path) -> Result<Option<String>> {
        if !path.exists() {
            return Ok(None);
        }
        let value = plist::Value::from_file(path)
            .with_context(|| format!("无法解析 {}", path.display()))?;
        Ok(value
            .as_dictionary()
            .and_then(|d| d.get("ProgramArguments"))
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_string())
            .map(str::to_string))
    }

    /// 开机自启当前是否「真的」会生效:下次登录会不会启动一个确实存在的程序。
    ///
    /// 这**不是**「我们有没有注册过开机自启」——注册项(plist 文件)存在,但它指向
    /// 的可执行文件已经不在了(app 被移动、重装到别的路径、或者整个删掉了),这里
    /// 也返回 `false`。原因:那条注册项已经是个死链,下次登录什么都不会启动,报
    /// `true` 就是在撒谎。
    ///
    /// 一个容易踩的坑:注册项存在**不代表**它指向的就是当前这次运行的程序。同一个
    /// app 完全可能有另一个副本在别处也能启动(比如开发期 `target/debug/` 下天天跑
    /// 的那份,或者用户下载了新版但没覆盖旧版),这种情况下注册项没坏、目标文件也
    /// 存在,只是不是「这一个」——所以这里不做「重写成 current_exe()」的自愈,那样
    /// 会把用户的登录项静默重定向到别的副本,还一声不吭。
    ///
    /// 副作用:app 被移动之后,设置里的开关会显示成「关」,需要用户手动重新勾选一次
    /// 才会用当前路径重新写入。这是有意为之——好过在用户不知情的情况下静默篡改登录项。
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
                .with_context(|| format!("无法写入 {}", path.display()))?;
        } else if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("无法删除 {}", path.display()))?;
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_missing_plist_has_no_registered_target() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("nope.plist");
            assert_eq!(registered_target_at(&path).unwrap(), None);
        }

        #[test]
        fn parses_the_first_program_argument() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("{LABEL}.plist"));
            std::fs::write(&path, plist_xml("/Applications/gateterm.app/Contents/MacOS/gateterm"))
                .unwrap();
            assert_eq!(
                registered_target_at(&path).unwrap().as_deref(),
                Some("/Applications/gateterm.app/Contents/MacOS/gateterm")
            );
        }

        #[test]
        fn a_malformed_plist_is_an_error() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("broken.plist");
            std::fs::write(&path, b"this is not xml").unwrap();
            assert!(registered_target_at(&path).is_err());
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::path::Path;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "gateterm";

    /// 去掉字符串两端各一个双引号(如果都在的话)。写入 Run 键时给路径加了引号
    /// (见 `set`),读出来比较之前要先去掉,不然永远匹配不上。
    fn strip_quotes(s: &str) -> &str {
        s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s)
    }

    /// 读出 Run 键里注册的可执行文件路径(已去掉引号)。没有这个键/值时是 `Ok(None)`。
    fn registered_target() -> Result<Option<String>> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(run) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ) else {
            return Ok(None);
        };
        Ok(run.get_value::<String, _>(VALUE).ok().map(|v| strip_quotes(&v).to_string()))
    }

    /// 开机自启当前是否「真的」会生效:下次登录会不会启动一个确实存在的程序。
    ///
    /// 这**不是**「我们有没有注册过开机自启」——Run 键的值存在,但它指向的可执行
    /// 文件已经不在了(app 被移动、重装到别的路径、或者整个删掉了),这里也返回
    /// `false`。原因:那条注册项已经是个死链,下次登录什么都不会启动,报 `true`
    /// 就是在撒谎。
    ///
    /// 一个容易踩的坑:注册项存在**不代表**它指向的就是当前这次运行的程序。同一个
    /// app 完全可能有另一个副本在别处也能启动(比如开发期 `target/debug/` 下天天跑
    /// 的那份,或者用户下载了新版但没覆盖旧版),这种情况下注册项没坏、目标文件也
    /// 存在,只是不是「这一个」——所以这里不做「重写成 current_exe()」的自愈,那样
    /// 会把用户的登录项静默重定向到别的副本,还一声不吭。
    ///
    /// 副作用:app 被移动之后,设置里的开关会显示成「关」,需要用户手动重新勾选一次
    /// 才会用当前路径重新写入。这是有意为之——好过在用户不知情的情况下静默篡改登录项。
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
            .context("无法打开 Run 注册表键")?;
        if enabled {
            // 加引号:路径里有空格时 Windows 才不会把它当成多个参数
            run.set_value(VALUE, &format!("\"{}\"", exe_path()?))
                .context("无法写入自启项")?;
        } else {
            match run.delete_value(VALUE) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).context("无法删除自启项"),
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn strips_a_matching_pair_of_quotes() {
            assert_eq!(strip_quotes(r#""C:\path\to\exe.exe""#), r"C:\path\to\exe.exe");
        }

        #[test]
        fn leaves_an_unquoted_value_alone() {
            assert_eq!(strip_quotes(r"C:\path\to\exe.exe"), r"C:\path\to\exe.exe");
        }

        #[test]
        fn does_not_strip_a_single_leading_quote() {
            // 只有一头有引号说明这不是我们写入的那种格式,原样返回,不瞎猜。
            assert_eq!(strip_quotes(r#""C:\odd"#), r#""C:\odd"#);
        }
    }
}

/// Linux 用 XDG Autostart:`$XDG_CONFIG_HOME/autostart/<name>.desktop`。GNOME、KDE、
/// 以及 Omarchy / Hyprland 这类用 `uwsm` / `dex` / systemd-xdg-autostart-generator
/// 的会话都认这个目录。
#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::path::{Path, PathBuf};

    const FILE_NAME: &str = "gateterm-desktop-web.desktop";

    /// desktop entry 规范里 Exec 的引号规则:整段用双引号包住,里面的 `"` `` ` `` `$` `\`
    /// 各自用反斜杠转义;`%` 是字段码前缀,要写成 `%%`。
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

    /// `exec_quote` 的逆运算,只认我们自己写出去的那种格式;不带引号时取第一个空白前的部分。
    fn exec_unquote(exec: &str) -> String {
        let exec = exec.trim();
        let Some(rest) = exec.strip_prefix('"') else {
            return exec.split_whitespace().next().unwrap_or_default().replace("%%", "%");
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
             Name=Gateterm Desktop Web\n\
             Comment=gateterm tray service\n\
             Exec={exec}\n\
             Icon=gateterm\n\
             Terminal=false\n\
             NoDisplay=true\n\
             X-GNOME-Autostart-enabled=true\n\
             X-Gateterm-Label={label}\n",
            exec = exec_quote(exe),
            label = LABEL,
        )
    }

    fn entry_path() -> Result<PathBuf> {
        let base = directories::BaseDirs::new().context("无法确定 HOME")?;
        Ok(base.config_dir().join("autostart").join(FILE_NAME))
    }

    /// 解析 desktop 文件里 `Exec=` 指向的可执行文件。文件不存在是 `Ok(None)`。
    fn registered_target_at(path: &Path) -> Result<Option<String>> {
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("无法读取 {}", path.display()))?;
        // `Hidden=true` 是规范里「用户在会话设置里关掉了它」的写法,等同于没注册。
        if text.lines().any(|l| l.trim() == "Hidden=true") {
            return Ok(None);
        }
        Ok(text
            .lines()
            .find_map(|l| l.trim().strip_prefix("Exec="))
            .map(exec_unquote)
            .filter(|s| !s.is_empty()))
    }

    /// 语义同 macOS / Windows:注册项存在**且**指向的文件还在,才算开。
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
                .with_context(|| format!("无法写入 {}", path.display()))?;
        } else if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("无法删除 {}", path.display()))?;
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn round_trips_a_plain_path() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(FILE_NAME);
            std::fs::write(&path, desktop_entry("/home/me/.local/bin/gateterm-desktop-web")).unwrap();
            assert_eq!(
                registered_target_at(&path).unwrap().as_deref(),
                Some("/home/me/.local/bin/gateterm-desktop-web")
            );
        }

        #[test]
        fn round_trips_a_path_with_special_characters() {
            let exe = r#"/opt/my apps/50% "gt"/$x\gateterm"#;
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(FILE_NAME);
            std::fs::write(&path, desktop_entry(exe)).unwrap();
            assert_eq!(registered_target_at(&path).unwrap().as_deref(), Some(exe));
        }

        #[test]
        fn a_missing_or_hidden_entry_is_not_registered() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(FILE_NAME);
            assert_eq!(registered_target_at(&path).unwrap(), None);
            std::fs::write(&path, desktop_entry("/usr/bin/gateterm-desktop-web") + "Hidden=true\n").unwrap();
            assert_eq!(registered_target_at(&path).unwrap(), None);
        }
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
compile_error!("gateterm-desktop-web 只支持 Windows、macOS 与 Linux");

/// `is_enabled` 的语义见其定义处(`imp` 模块内,各平台一份,内容相同)。
pub use imp::{is_enabled, set};

#[cfg(target_os = "linux")]
pub use imp::desktop_entry;

#[cfg(target_os = "macos")]
pub use imp::plist_xml;
