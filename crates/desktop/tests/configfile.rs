use octoterm_desktop::configfile::{save, Editable};
use octoterm_desktop::configfile::load_language;
use octoterm_desktop::i18n::LanguagePref;

fn edit(listen: &str, token: Option<&str>) -> Editable {
    Editable { listen: listen.parse().unwrap(), token: token.map(String::from), language: Default::default() }
}

#[test]
fn language_round_trips_without_losing_other_desktop_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "# keep this\n[desktop]\n# keep this too\nfuture_option = true\n").unwrap();
    for language in LanguagePref::ALL {
        let mut next = edit("127.0.0.1:7683", Some("fixed"));
        next.language = language;
        save(&path, &next).unwrap();
        assert_eq!(load_language(&path), language);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep this too"));
        assert!(text.contains("future_option = true"));
        // The shared server configuration must still load with desktop settings.
        let config = octoterm_server::config::Config::load(Some(path.clone())).unwrap();
        assert_eq!(config.token.as_deref(), Some("fixed"));
    }
}

#[test]
fn absent_invalid_or_unknown_language_follows_system() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    assert_eq!(load_language(&path), LanguagePref::Auto);
    for text in ["", "listen = '127.0.0.1:7683'", "invalid = =", "[desktop]\nlanguage = 'unknown'", "[desktop]\nlanguage = 3"] {
        std::fs::write(&path, text).unwrap();
        assert_eq!(load_language(&path), LanguagePref::Auto);
    }
}

#[test]
fn invalid_desktop_section_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = "desktop = 'custom'\n";
    std::fs::write(&path, original).unwrap();
    assert!(save(&path, &edit("127.0.0.1:7683", None)).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn preserves_comments_and_launcher_sections() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        r#"# 我手写的注释,不许动
listen = "127.0.0.1:7683"

# 启动项
[[launcher]]
name = "prod ssh"
command = ["ssh", "prod01"]
"#,
    )
    .unwrap();

    save(&path, &edit("127.0.0.1:9000", None)).unwrap();

    let out = std::fs::read_to_string(&path).unwrap();
    assert!(out.contains("# 我手写的注释,不许动"), "注释被碾掉了:\n{out}");
    assert!(out.contains("# 启动项"), "段注释被碾掉了:\n{out}");
    assert!(out.contains(r#"name = "prod ssh""#), "launcher 段丢了:\n{out}");
    assert!(out.contains(r#"listen = "127.0.0.1:9000""#), "listen 没改:\n{out}");
}

#[test]
fn creates_a_minimal_file_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("config.toml");

    save(&path, &edit("0.0.0.0:7683", Some("fixed"))).unwrap();

    let out = std::fs::read_to_string(&path).unwrap();
    assert!(out.contains(r#"listen = "0.0.0.0:7683""#), "{out}");
    assert!(out.contains(r#"token = "fixed""#), "{out}");
}

#[test]
fn clearing_the_token_removes_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "listen = \"127.0.0.1:7683\"\ntoken = \"old\"\n").unwrap();

    save(&path, &edit("127.0.0.1:7683", None)).unwrap();

    let out = std::fs::read_to_string(&path).unwrap();
    assert!(!out.contains("token"), "token 键应当被移除:\n{out}");
}

#[test]
fn a_broken_file_is_reported_and_left_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let broken = "listen = = =\n";
    std::fs::write(&path, broken).unwrap();

    assert!(save(&path, &edit("127.0.0.1:9000", None)).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), broken, "坏文件不该被改写");
}

/// 用 unix 权限位模拟「文件存在但读取失败」(权限不足、路径其实是目录等)。
/// Windows 的 ACL 权限模型没有对应的 rwx 位可以直接 chmod 出同样的
/// PermissionDenied,所以这个场景只在 unix 上覆盖。
#[cfg(unix)]
#[test]
fn unreadable_file_is_reported_and_left_untouched() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = "# 我手写的注释,不许动\nlisten = \"127.0.0.1:7683\"\n";
    std::fs::write(&path, original).unwrap();

    // 去掉读权限、只留写权限:rename 只需要目录的写权限就能成功,
    // 但 read_to_string 会失败 —— 这正是要抓的场景。
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o200)).unwrap();

    let result = save(&path, &edit("127.0.0.1:9000", None));

    // 无论上面断言是否通过,先把权限改回去,否则 tempdir 清理会失败。
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert!(result.is_err(), "读取失败应当报错,而不是被当成空文件处理");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "读不出来的文件不该被静默清空"
    );
}

#[test]
fn no_temp_file_is_left_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    save(&path, &edit("127.0.0.1:7683", None)).unwrap();

    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "残留临时文件:{leftovers:?}");
}
