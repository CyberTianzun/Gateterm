//! 弹给用户看的模态提示。
//!
//! GUI 进程没有可见的 stderr(见 [`crate::logs`] 的模块注释),启动期的失败如果
//! 只写日志,用户看到的就是「双击了没反应」——而最坏的一种失败恰恰是日志自己没
//! 建起来,那时候连日志都不存在。这种时候只剩弹框这一条路。
//!
//! 这里的函数都是同步模态的:弹着的时候调用线程整个卡住,后台服务继续运行。

/// 报一条致命错误。调用方在这之后应当结束进程 —— 不要报完继续跑。
pub fn fatal(title: &str, body: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(title)
        .set_description(body)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// 报一条中性提示,不代表出错。
pub fn notice(title: &str, body: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Info)
        .set_title(title)
        .set_description(body)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// Returns true only when the user chooses to open the new release in their browser.
pub fn confirm_update(
    current_version: &str,
    update: &octoterm_server::update::ReleaseUpdate,
) -> bool {
    const UPDATE_LABEL: &str = "立刻更新";
    let result = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Info)
        .set_title("octoterm 有新版本")
        .set_description(format!(
            "发现新版本 {} (当前版本 {})。\n\n点击“立刻更新”将在浏览器中打开 Release 页面。\n\n{}",
            update.version, current_version, update.url
        ))
        .set_buttons(rfd::MessageButtons::OkCancelCustom(
            UPDATE_LABEL.into(),
            "取消".into(),
        ))
        .show();
    matches!(result, rfd::MessageDialogResult::Custom(ref label) if label == UPDATE_LABEL)
        || result == rfd::MessageDialogResult::Ok
}
