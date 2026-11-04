# Gateterm iOS / iPadOS 客户端

SwiftUI + UIKit 原生客户端,功能对齐 `clients/android`(连接管理、会话、终端、Agent 状态/聊天/历史、
Cloud 账号),交互按 iOS 规范重新设计,iPad 宽屏下是带侧边栏的三栏布局。

终端内核与 Android 共用 Rust `crates/client-core`(`alacritty_terminal`),Cloud 账号逻辑共用
`crates/cloud-client`,都经 `crates/cloud-ffi` 的 UniFFI 导出;Swift 侧只做渲染、输入和界面。

## 构建

前置:Xcode 26、[XcodeGen](https://github.com/yonaskolb/XcodeGen)(`brew install xcodegen`)、Rust 及 iOS 目标:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
scripts/build-ios-ffi.sh          # 首次:生成 clients/ios/Generated/(XCFramework + Swift 绑定)
cd clients/ios && xcodegen        # 由 project.yml 生成 Gateterm.xcodeproj
open Gateterm.xcodeproj
```

之后 Xcode 每次构建都会以 `--if-stale` 调用 `build-ios-ffi.sh`,Rust 源码没变时直接跳过。
`Generated/`、`Gateterm.xcodeproj`、`Info.plist` 都是生成物,不入库。

- 真机:在 Signing & Capabilities 里选择 Team。推送需要 Push Notifications 能力(`Gateterm.entitlements`
  已声明 `aps-environment`)。
- 模拟器:需要在 Xcode ▸ Settings ▸ Components 里安装 iOS 模拟器运行时;没有任何运行时时
  `actool` 编译资源目录会失败(Xcode 26 的已知行为)。
- Debug 构建支持两个启动参数,方便在模拟器里免输入联调(Release 不含这段代码):

  ```sh
  xcrun simctl launch booted ai.eclosion.gateterm.ios \
    -GatetermSeedURL "http://127.0.0.1:48731/#token=t" -GatetermOpenSession YES
  ```

### 打包

版本号取自 `project.yml` 的 `MARKETING_VERSION` / `CURRENT_PROJECT_VERSION`,与根 `Cargo.toml`
的 `version` / `version_code` 保持一致。

```sh
cd clients/ios
xcodebuild -project Gateterm.xcodeproj -scheme Gateterm -configuration Release \
  -destination 'generic/platform=iOS' -archivePath build/Gateterm.xcarchive \
  DEVELOPMENT_TEAM=<Team ID> -allowProvisioningUpdates archive
xcodebuild -exportArchive -archivePath build/Gateterm.xcarchive -exportPath build/export \
  -exportOptionsPlist ExportOptions.plist -allowProvisioningUpdates
```

`ExportOptions.plist` 的 `method` 用 `debugging`(装到已登记设备)、`release-testing`(Ad Hoc)
或 `app-store-connect`(TestFlight / App Store)。

### 测试

```sh
clients/ios/scripts/test-host.sh
```

在 Mac 本机直接编译与平台无关的源码(协议、URL/二维码、键盘编码、聊天合并、外观、Agent 布局、
Rust 终端内核)并用 XCTest 运行,不需要模拟器。用例从 Android 单测移植,保证两端编码逐字节一致。
设置 `GATETERM_TEST_URL` 后会额外跑端到端用例(握手、新建会话、attach、输入回显、错误 token):

```sh
gateterm-server --host 127.0.0.1 --port 48731 --token t &
GATETERM_TEST_URL="http://127.0.0.1:48731/#token=t" clients/ios/scripts/test-host.sh
```

同一批测试文件也挂在 Xcode 的 `GatetermTests` 目标上(Product ▸ Test)。

## 与 Android 版的对应和 iOS 化改动

| Android | iOS / iPadOS |
| --- | --- |
| 单 Activity + 屏幕栈 | `NavigationSplitView` 三栏:服务器(侧边栏)→ 会话 → 终端;iPhone 自动折叠成导航栈,支持边缘右滑返回 |
| 右下角 FAB「+」 | 导航栏右上角「+」菜单(扫码 / 剪贴板 / 手动) |
| 行尾 ⋮ 溢出菜单 | 左右滑动操作 + 长按上下文菜单 |
| 编辑器全屏页,底部保存按钮 | 表单 sheet,左上「取消」、右上「保存」;有输入时禁止下滑误关 |
| AlertDialog 删除确认 | `confirmationDialog`(操作表) |
| 设置 / Cloud 页 | 带「完成」按钮的模态表单;Agent 快捷键布局为可拖动排序、左滑移除的列表 |
| 历史:独立筛选页 + 刷新按钮 | 搜索栏 + 工具栏筛选菜单 + 下拉刷新 + 推入会话预览 |
| Snackbar | 顶部胶囊提示 + 触感反馈 |
| 快捷键栏 + ‹ › 翻页 | 跟随键盘的横向滚动快捷键栏(默认单行,可改双行) |
| 浏览器 + 自定义 scheme 回调登录 | `ASWebAuthenticationSession`(系统登录页,回调由系统截获) |
| token 存 SharedPreferences | token 存 Keychain(仅本机),其余元数据存 UserDefaults |

iPad 额外:

- 打开会话后自动收起服务器侧边栏,给终端让出宽度;可随时用系统按钮展开。
- 多个已打开会话在终端顶部显示为标签,点切换、× 关闭(本地最多 8 个)。
- 外接键盘:方向键、Esc、Tab、F1–F12、Ctrl/Alt 组合直接编码;⌘V 粘贴走 bracketed paste;
  服务端为 Windows 且开启 win32-input-mode 时按键按下/抬起分别上报。
- 输入法候选窗跟随终端光标位置。

终端手势:轻点呼出键盘,单指拖动翻历史(带惯性),双指拖动始终滚动,双指捏合调整字号,
长按拖选文字后弹出系统「复制/粘贴」菜单;程序开启鼠标上报时单指操作转为鼠标事件。

## 已知差异与未完成

- **后台连接**:iOS 不允许像 Android 前台服务那样常驻 WebSocket。进入后台时 detach 终端,
  回到前台立即重连并通过 replay/resync 恢复;后台提醒依赖 Cloud 推送。
- **推送**:客户端已实现 APNs 设备注册(`register_push_target("apns", token)`)和点通知直达会话,
  但当前 Cloud 只会下发 `fcm` / `aliyun` 配置,iOS 会显示「Cloud 尚未为 iOS 开启 APNs」。
  需要 Cloud 侧支持 `apns` 平台并作为 APNs provider 下发可见通知,payload 带
  `server_id` / `session` / `agent_id` / `kind`。
- **Cloud 登录回调**沿用 Cloud 已允许的 `ai.eclosion.gateterm.android://cloud/auth/callback`。
  若 Cloud 将来为 iOS 单独登记 redirect URI,只需改 `CloudAccountModel.swift` 里的常量。
- 跨服务器的「Agent Inbox」、通知上直接审批等需求草案(`docs/requirements/2026-08-20-ios-client-and-cloud.md`)
  里的后续阶段内容未实现,与 Android 当前范围一致。
