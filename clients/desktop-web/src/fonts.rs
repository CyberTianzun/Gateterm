//! 界面字体。
//!
//! egui 自带的默认字体(Ubuntu-Light / Hack / NotoEmoji)**一个汉字都没有**,
//! 中文界面和语言选择器需要补一份中文字体,否则汉字会变成豆腐块。
//!
//! 不内嵌字体文件:一份覆盖常用汉字的字体动辄十几 MB,而 Windows 与 macOS 都必然
//! 自带,Linux 桌面发行版也几乎都会装 Noto CJK / 文泉驿。这里按候选列表逐个试,第一个
//! 读得出来的胜出;全都读不到时
//! 退回 egui 的默认字体 —— 中文会变成方块,但程序照常能用,不为了字体 panic。
//!
//! 插进去的是**最低优先级的 fallback**:拉丁字母和数字仍然由 egui 默认字体渲染
//! (它们的字形本来就更适合界面),只有默认字体里查不到的字符才落到中文字体上。

/// 字体在 `FontDefinitions` 里的键名。只有一份,所以是常量。
const KEY: &str = "system-cjk";

/// (路径, 字体集合里的第几个 face)。顺序即优先级。
#[cfg(target_os = "macos")]
const CANDIDATES: &[&str] = &[
    // 系统 UI 字体,10.11 起自带
    "/System/Library/Fonts/PingFang.ttc",
    // 老系统 / 精简安装的兜底
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/Supplemental/Songti.ttc",
];

#[cfg(windows)]
const CANDIDATES: &[&str] = &[
    // 微软雅黑,Vista 起自带
    r"C:\Windows\Fonts\msyh.ttc",
    r"C:\Windows\Fonts\msyh.ttf",
    r"C:\Windows\Fonts\simhei.ttf",
    r"C:\Windows\Fonts\simsun.ttc",
];

#[cfg(target_os = "linux")]
const CANDIDATES: &[&str] = &[
    // Arch / Omarchy:noto-fonts-cjk
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    // Debian / Ubuntu:fonts-noto-cjk
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    // Fedora:google-noto-sans-cjk-fonts
    "/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    // 文泉驿微米黑
    "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/wqy-microhei/wqy-microhei.ttc",
    // 思源黑体
    "/usr/share/fonts/adobe-source-han-sans/SourceHanSansCN-Regular.otf",
];

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
const CANDIDATES: &[&str] = &[];

/// 读出第一个可用的中文字体文件。返回 (路径, 内容)。
pub fn load() -> Option<(&'static str, Vec<u8>)> {
    CANDIDATES
        .iter()
        .find_map(|path| std::fs::read(path).ok().map(|bytes| (*path, bytes)))
}

/// .ttc 是字体集合,一般取第 0 个 face(常规字重)。Noto Sans CJK 的集合按
/// JP / KR / SC / TC / HK 排列,第 0 个是日文字形,简体中文界面要取第 2 个(SC)。
fn face_index(path: &str) -> u32 {
    if path.ends_with("NotoSansCJK-Regular.ttc") { 2 } else { 0 }
}

/// 把中文字体挂到 `ctx` 上。必须在第一帧之前调用。
pub fn install(ctx: &egui::Context) {
    let Some((path, bytes)) = load() else {
        tracing::warn!("没找到可用的中文字体,界面里的中文会显示成方块");
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    let mut data = egui::FontData::from_owned(bytes);
    data.index = face_index(path);
    fonts.font_data.insert(KEY.to_owned(), std::sync::Arc::new(data));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        // push 而不是 insert(0,…):放最后 = 只在默认字体查不到字形时才用它。
        fonts.families.entry(family).or_default().push(KEY.to_owned());
    }
    ctx.set_fonts(fonts);
    tracing::info!(font = path, "已加载中文字体");
}
