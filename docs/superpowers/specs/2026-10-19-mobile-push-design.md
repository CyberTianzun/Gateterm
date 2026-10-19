# 手机推送(Android FCM)与 Cloud 部署设计 spec

日期:2026-10-19
状态:已与 hiro 逐段对齐,等待书面 Review
上游:`~/online/eclosion/OctotermCloud/docs/specs/2026-10-19-octoterm-cloud-account-push-design.md`(下称「账号 spec」,已实现并合入两仓 dev)

## 0. 一句话

agent 在等人时,手机收到一条系统通知;在别处处理掉后通知自动撤回;点开直接连上对应 server 并跳到该会话。Android 走 FCM,iOS(APNs)留口子。账号逻辑用 UniFFI 复用 Rust `cloud-client`,三端行为一致。Cloud 以 Docker 镜像部署到 futures/ops,复用机上已有的 nginx。

## 1. 已确认的裁决

| # | 裁决 |
|---|---|
| M1 | 手机用 `server_id` 找本地连接:server 在 `HelloOk` 里回带 `cloud_server_id`,App 连上时记到该连接条目 |
| M2 | 手机端账号能力与 desktop 对齐(注册/登录/找回/退出/删除),满足商店「App 内可删账号」要求 |
| M3 | 账号逻辑不在 Kotlin 重写:新 crate `cloud-ffi` 用 UniFFI 导出 `cloud-client::ops`,iOS 将来复用 |
| M4 | FCM 只发高优先级**纯数据消息**,文案在 App 本地生成;Google 看不到可读内容 |
| M5 | 在别处处理后撤回通知(`kind=resolved`);待授权通知另带过期时间,到期自动消失 |
| M10 | 点通知只进入对应会话的终端;审批面板另起 |
| M6 | Android 只构建 arm64-v8a(只在真机测试) |
| M7 | iOS/APNs 留口子:`apns` 平台继续走 stub,Apple 账号就绪后另起 spec |
| M8 | Cloud 部署到 futures/ops,复用机上已有的 nginx(`ace-nginx`,host 网络);TLS 用复制到 `/opt/nginx/tls/signal.top/` 的 `*.signal.top` 证书;域名 `octoterm.signal.top` |
| M9 | 可迁移:Cloud 相关的一切在 git(compose、nginx 站点文件、文档),服务器上只有数据卷、stack 环境变量、证书文件、一个 nginx 站点文件 |

## 2. 数据流

```
 agent hook ─► octoterm-server notifier
                 进入 waiting  → POST /v1/notify {kind: waiting|permission, expires_at?}
                 离开 waiting  → POST /v1/notify {kind: resolved}
                      │
                      ▼
               OctotermCloud ─► FcmSender ─► FCM HTTP v1 ─► Android FirebaseMessagingService
                                ApnsSender(stub)              ├ waiting/permission → 本地通知(tag = server_id:session:agent_id)
                                                              └ resolved → 按 tag 撤回
 用户点通知 ─► MainActivity(server_id, session) ─► 找 cloud_server_id 匹配的连接 ─► 连接 ─► 打开该会话的终端
```

## 3. octoterm-server

### 3.1 `HelloOk.cloud_server_id`

- `ServerMsg::HelloOk` 新增 `#[serde(default, skip_serializing_if = "Option::is_none")] cloud_server_id: Option<String>`。新增可选字段,客户端忽略未知字段,**不 bump proto**。
- 值来自 `cloud.json` 的 `server_id`;未登录为 `None`。读取与通知器共用一个按 `(mtime, len)` 缓存的凭据读取器(把 `notifier.rs` 里的 `CredCache` 提为 `cloud::creds` 模块,`Arc<Mutex<_>>` 共享),握手路径只做一次 `stat`。
- web 客户端不使用此字段,无需改动。

### 3.2 通知器规则(在现有 `classify` 上扩展)

| 前一状态 | 本次 | 推送 |
|---|---|---|
| 非 waiting | waiting(无 pending) | `waiting` |
| 任意 | waiting + 新 pending | `permission`,带 `expires_at` = 该 PendingRequest 的 `expires_at` |
| waiting(且此前推送过) | 非 waiting,或会话被清理 | `resolved` |
| 其余 | — | 不推 |

- 「此前推送过」以通知器内存表为准:只有真的发过 `waiting/permission` 的会话才发 `resolved`,避免对从未通知过的会话发撤回。
- `resolved` 不带 `tool_name`、`expires_at`。

## 4. OctotermCloud

### 4.1 `/v1/notify` 扩展

- `kind` 新增 `resolved`。
- 新增可选 `expires_at`(unix 秒):必须大于当前时间,且不超过当前时间 + 3600,否则 `400 invalid_expires_at`。
- `PushMessage` 增加 `expires_at: Option<i64>`。

### 4.2 推送发送器

- `PushSender::send` 的错误类型改为 `PushError { InvalidToken, Other(anyhow::Error) }`。`/notify` 遇到 `InvalidToken` 删除对应 `push_targets` 行(不计入 delivered)。
- 按平台分派:`fcm` → `FcmSender`(配置存在时)否则 `LogSender`;`apns` → `LogSender`(M7)。
- **FcmSender**:
  - 凭据:`FCM_SERVICE_ACCOUNT_JSON`(base64 编码的服务账号 JSON,首选,部署时不落文件)或 `FCM_SERVICE_ACCOUNT_FILE`。都没有时不启用 FCM,启动日志说明。
  - 鉴权:用服务账号私钥签 RS256 JWT(`iss`=client_email,`scope`=`https://www.googleapis.com/auth/firebase.messaging`,`aud`=token_uri),换 OAuth access token;缓存到过期前 5 分钟,并发请求共用一次刷新。
  - 请求:`POST https://fcm.googleapis.com/v1/projects/{project_id}/messages:send`
    ```json
    {"message": {
      "token": "<push_token>",
      "data": {"server_id": "...", "session": "7", "agent_id": "claude-code", "kind": "permission",
               "tool_name": "Bash", "expires_at": "1760000000"},
      "android": {"priority": "HIGH", "collapse_key": "<server_id>:<session>:<agent_id>", "ttl": "<秒>s"}
    }}
    ```
    data 值一律字符串;`tool_name`/`expires_at` 缺省时不出现;`ttl` 仅在有 `expires_at` 时设置(= 剩余秒数),否则用 FCM 默认。`collapse_key` 带上 `agent_id`,避免手机离线期间一个 agent 的 `resolved` 顶掉另一个 agent 还没收到的 `waiting`;`kind == "resolved"` 时 `priority` 为 `"NORMAL"`(其余为 `"HIGH"`),因为它只撤销通知、从不展示新通知,避免 FCM 因高优先级消息不产生可见通知而降权该 app。
  - 错误:错误详情含 `UNREGISTERED` → `InvalidToken`;401/403(凭据问题)、429、5xx、裸 404(无 `UNREGISTERED`,如 `FCM_API_BASE` 配错或代理返回的 404)→ `Other`,只记日志不重试(撤回 + 过期保证不会留下错误状态)。
  - 日志不得含 push_token、tool_name(沿用账号 spec 的隐私规则,隐私测试覆盖 FcmSender 路径)。

### 4.3 备份子命令

`octoterm-cloud backup <path>`:用 `VACUUM INTO` 生成一致性快照。用于迁移与定期备份。

## 5. `crates/cloud-ffi`(UniFFI)

- crate-type `["cdylib", "staticlib"]`;UniFFI proc-macro 方式;异步函数 `#[uniffi::export(async_runtime = "tokio")]`。
- 导出对象 `CloudAccount(cred_path: String, label: String)`,方法与 `ops` 一一对应:`request_code(email, purpose)`、`register(url, email, code, password)`、`login(url, email, password)`、`reset_password(url, email, code, new_password)`、`logout()`、`request_delete_code()`、`delete_account(password, code)`、`status() -> Option<CloudStatus{email, url}>`(只读本地文件)、`register_push_target(platform, token)`。
- 登录/注册以 `kind = device` 进行:`ops::register/login` 增加 `kind: ClientKind` 参数(server CLI 与 desktop 传 `Server`)。
- 新增 `ops::register_push_target(path, platform, token)`,走既有 `revoke_on_401`。
- 错误导出为枚举 `CloudFfiError { NotLoggedIn, Unauthorized, RateLimited{retry_after}, Api{code}, Unreachable, Local }`——**只给错误码,不给中文文案**;文案由 App 按自身语言(已有中英 i18n)生成。

## 6. Android App

### 6.1 构建

- Gradle 任务 `buildCloudFfi`:`cargo ndk -t arm64-v8a -o app/src/main/jniLibs build --release -p octoterm-cloud-ffi`,随后 `uniffi-bindgen generate --language kotlin` 输出到 `app/build/generated/uniffi`,加入 Kotlin 源集;`preBuild` 依赖它。NDK 路径取 `ANDROID_NDK_HOME`(本机 `/Users/hiro/frameworks/android-ndk-r30`)。
- 依赖:`net.java.dev.jna:jna:<ver>@aar`(UniFFI Kotlin 需要)、Firebase BoM + `firebase-messaging`(**不加 analytics**)、`com.google.gms.google-services` 4.5.0 插件。
- `app/google-services.json` 列入 `.gitignore`;**文件不存在时不应用 google-services 插件**,开源构建照常可用,只是没有推送。本机文件来源:`~/online/google-services.json`。

### 6.2 Cloud 账号界面

- 设置里新增「Cloud 账号」页,能力与 desktop 一致;凭据文件 `filesDir/cloud.json`,label = 设备型号。
- 登录成功后:请求 `POST_NOTIFICATIONS`(Android 13+)→ 取 FCM token → `register_push_target("fcm", token)`;`onNewToken` 时若已登录重新登记。
- 用户拒绝通知权限:页内显示「通知已关闭」与跳转系统设置。

### 6.3 连接与 `cloud_server_id`

- `ServerConnection` 增加可选 `cloudServerId`;`wire/Messages.kt` 解析 `HelloOk.cloud_server_id`;每次连接成功写回该条目(为空则清空)。

### 6.4 通知

- `OctoMessagingService : FirebaseMessagingService`,渠道 `agent_waiting`(高重要度)。
- `waiting`/`permission`:`tag = "<server_id>:<session>:<agent_id>"`,同会话同 agent 覆盖(不同 agent 互不撤回);文案本地化生成(如「Claude Code 在等你」/「Claude Code 请求授权:Bash」);有 `expires_at` 时 `setTimeoutAfter(剩余毫秒)`。
- `resolved`:`cancel(tag, id)`。
- 文案生成、tag 规则、`server_id → 连接` 查找写成纯函数并单测。

### 6.5 点击通知

- PendingIntent → `MainActivity`(extras: `server_id`, `session`, `kind`)。
- 找到 `cloudServerId` 匹配的连接 → 连接 → 进入该会话的终端(用户在终端里回答;审批面板不在本期)。
- 找不到连接:进入首页并提示「请先连接一次这台 server」;连接失败:显示错误,通知保留。

## 7. 部署(futures/ops)

### 7.0 ops 现状(2026-10-19 经 Portainer API 只读核实)

- Portainer 环境 `futures/ops`(endpoint 15)是**本地 Docker 环境**(type 1),不是 Edge 环境 → 用普通 compose stack,不用 Edge Stack。
- 反代是容器 `ace-nginx`(podman-compose,工作目录 `/opt/nginx`,**host 网络**):
  `/opt/nginx/conf.d` → `/etc/nginx/conf.d`(ro),`/opt/nginx/tls` → `/etc/nginx/tls`(ro)。
- 惯例:各服务只把端口发布到 `127.0.0.1:<port>`,由 nginx 反代。`18080` 已被 headscale 占用。

### 7.1 镜像

- OctotermCloud 仓库(私有,`git@github.com:CyberTianzun/OctotermCloud.git`)加 GitHub Actions:构建 Dockerfile,推私有镜像仓库 `registry.hnminigames.fun/octoterm-cloud`(dev 分支 → `dev-<sha>`;`v*` tag → 同名 tag 与 `latest`)。本机无 docker,由 CI 首次验证镜像构建。
- CI 登录凭据只放在 GitHub Actions secrets `REGISTRY_USERNAME` / `REGISTRY_PASSWORD`,不进仓库、不进 workflow 明文。
- ops 上其它容器已从该 registry 拉镜像(Portainer 已有拉取凭据,部署时确认)。

### 7.2 Stack

- `deploy/compose.yml`(入 git)。部署时由 Portainer API 在 endpoint 15 创建普通 stack `octoterm-cloud`,内容取自该文件:
  - 服务 `octoterm-cloud`:镜像 `registry.hnminigames.fun/octoterm-cloud:${OCTOTERM_CLOUD_TAG}`;端口 `127.0.0.1:${OCTOTERM_CLOUD_PORT:-18090}:8080`;命名卷 `cloud-data:/data`;环境变量 `RESEND_API_KEY`、`MAIL_FROM`、`FCM_SERVICE_ACCOUNT_JSON`、`TRUST_PROXY=1` 在 stack 环境变量里设置。
- nginx 站点 `/opt/nginx/conf.d/octoterm.signal.top.conf`(原文入 git:`deploy/nginx/octoterm.signal.top.conf`):

```nginx
server {
    listen 443 ssl;
    server_name octoterm.signal.top;
    ssl_certificate     /etc/nginx/tls/signal.top/fullchain.pem;
    ssl_certificate_key /etc/nginx/tls/signal.top/privkey.pem;
    location / {
        proxy_pass http://127.0.0.1:18090;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

  `X-Forwarded-For` 一行是 `TRUST_PROXY=1` 的前提(nginx 默认不追加)。若 nginx 已有 80→443 的统一跳转则不另加 80 端口块,否则补一个只做跳转的 80 块(部署时看 `conf.d` 现有文件决定)。
- 证书:`*.signal.top` 从统一来源复制到 `/opt/nginx/tls/signal.top/`(`fullchain.pem`、`privkey.pem`),复用 nginx 已有挂载,无需改 nginx 的 compose。

### 7.3 写主机文件的方式

Portainer 不能直接写主机目录,且 `ace-nginx` 的挂载是只读的。做法:经 Portainer Docker API 起一个一次性辅助容器(复用 `ace-nginx` 的本地镜像,免拉取),以**读写**方式挂载 `/opt/nginx/conf.d` 与 `/opt/nginx/tls`,用 `PUT /containers/{id}/archive` 上传 conf 与证书,随后删除辅助容器;再在 `ace-nginx` 内 exec `nginx -t`,通过后 `nginx -s reload`。执行前把将写入的文件与命令给 hiro 过目。

### 7.4 迁移文档

`docs/migration.md`:服务器上属于 Cloud 的全部状态清单(数据卷、stack 变量名、证书目录、nginx 站点文件),以及「`backup` → 新机部署同一 stack → 恢复卷 → 放 conf 与证书 → 改 DNS」的步骤。

### 7.5 访问方式

部署经 Portainer API(`https://portainer.hnminigames.fun`)完成;API key 只在命令中从 `~/online/eclosion/portainer.txt` 读入变量,不打印、不进仓库。

## 8. 错误处理

- 推送尽力而为:FCM/Cloud 不可达只记日志,不影响 hook、WS。
- token 失效(`UNREGISTERED`)→ Cloud 删除登记;App 下次启动或 `onNewToken` 重登。
- 撤回推送丢失 → 待授权通知按 `expires_at` 自行消失;普通 waiting 通知留到用户点开(点开后若已无等待,直接进会话)。
- 手机端 401 `unauthorized` → Rust `revoke_on_401` 删本地凭据,UI 回到未登录(与 desktop/server 同一规则)。

## 9. 测试

- **Cloud**:假 FCM + 假 OAuth token 端点测 `FcmSender`(JWT 字段、token 缓存与并发刷新、请求体、`UNREGISTERED` → 删除登记);`/notify` 的 `resolved`/`expires_at` 校验;隐私测试覆盖 FCM 路径;`backup` 产出可打开的数据库。
- **server**:`classify` 迁移表(含 `resolved` 与「从未推送不撤回」);`expires_at` 透传;`HelloOk` 在登录/未登录两种情况下的 `cloud_server_id`。
- **cloud-ffi**:Rust 侧对导出对象做测试(对着假 Cloud:设备身份登录、登记 push target、401 吊销、错误码映射);生成的 Kotlin 绑定由 App 编译检查,行为由真机清单覆盖(JNA 的 aar 无法在纯 JVM 单测里加载)。
- **Android**:纯函数单测(文案、tag、连接匹配);真机手工清单:登录 → 电脑触发 agent 等待 → 手机收到通知 → 电脑处理 → 手机通知消失 → 再次触发 → 点通知进入该会话的终端 → 拒绝通知权限时的提示。
- **部署**:CI 镜像构建通过;部署后 `https://octoterm.signal.top/healthz` 返回 `ok`;用真实 FCM 发一条到测试机。

## 10. 不在本期

- iOS:APNs 真实发送、iOS 界面(Apple 账号就绪后另起 spec)。
- 锁屏直接批准/拒绝;App 内审批面板(当前 Android 只能在终端里回答)。
- Google Play 要求的网页删号页(记 TODO)。
- x86_64 模拟器构建。
