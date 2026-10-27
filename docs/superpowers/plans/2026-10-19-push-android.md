# 手机推送:Android 端 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Android App 能登录 Gateterm Cloud(设备身份)、登记 FCM token、收到 agent 等待推送时显示系统通知(别处处理后撤回、待授权到期消失),点通知连上对应 server 并打开该会话的终端。

**Architecture:** Rust `cloud-ffi` 由 Gradle 任务调用 `scripts/build-android-ffi.sh` 编出 arm64 `.so` 与 Kotlin 绑定(生成到 `build/`,不入库)。账号界面是独立的 `CloudAccountViewModel` + `CloudAccountScreen`(通过 `AppScreen.CloudAccount` 进入)。推送由 `GateMessagingService` 接收,纯函数 `AgentPush` 解析与生成文案,`AgentNotifications` 负责显示/撤回。连接记录 `cloudServerId`(来自 `HelloOk`),点通知时据此找回连接。

**Tech Stack:** Kotlin 2.1.21、AGP 8.13.2、Jetpack Compose、org.json、JUnit4、Firebase Messaging(BoM 35.0.2)、JNA 5.17.0(aar)、UniFFI Kotlin 绑定。

**Spec:** `docs/superpowers/specs/2026-10-19-mobile-push-design.md` §6、§8、§9(下称「推送 spec」)。
**依赖:** `docs/superpowers/plans/2026-10-19-push-server-and-ffi.md` 已完成(`crates/cloud-ffi`、`scripts/build-android-ffi.sh`、`HelloOk.cloud_server_id`)。

## Global Constraints

- 只构建 `arm64-v8a`;Gradle 通过 `scripts/build-android-ffi.sh` 构建 Rust,需要环境变量 `ANDROID_NDK_HOME`(本机 `/Users/hiro/frameworks/android-ndk-r30`)和 PATH 里的 `cargo`/`cargo-ndk`。
- **不加 Firebase Analytics**;只用 `firebase-messaging`。
- `app/google-services.json` 列入 `.gitignore`;**文件不存在时不应用 google-services 插件**,且运行期 `FirebaseApp.getApps(ctx).isEmpty()` 时推送功能整体降级(不崩溃)。本机来源 `~/online/google-services.json`。
- FCM 消息是纯数据消息,字段:`server_id`、`session`、`agent_id`、`kind`(waiting/permission/resolved;done 忽略)、可选 `tool_name`、可选 `expires_at`(unix 秒字符串)。
- 通知 tag = `"<server_id>:<session>:<agent_id>"`,同一会话里同一 agent 覆盖、不同 agent 互不影响;`resolved` 按 tag 撤回;有 `expires_at` 时 `setTimeoutAfter(剩余毫秒)`。
- 点通知只进入对应会话的终端(审批面板不在本期)。
- 凭据文件 `filesDir/cloud.json`;设备 label = `Build.MODEL`。
- 新文案同时加到 `res/values/strings.xml`(中文,默认)与 `res/values-en/strings.xml`(英文),key 一一对应。
- 现有架构:单一 `ConnectionViewModel` + 自定义 `AppScreen` 状态机,无 Navigation 组件;`org.json` 解析;JUnit4 纯 JVM 单测(无 Robolectric)。新的 Android 框架相关代码不写 JVM 单测,只把纯逻辑抽成可测函数。
- Gradle 命令(本机):
  - 首次拉新依赖(需要代理,Gradle JVM 不读 shell 的 `HTTPS_PROXY`):
    `G=$(ls -d ~/.gradle/wrapper/dists/gradle-8.14.3-bin/*/gradle-8.14.3/bin/gradle); $G -Dhttps.proxyHost=mm1.ztt.io -Dhttps.proxyPort=6157 -Dhttp.proxyHost=mm1.ztt.io -Dhttp.proxyPort=6157 assembleDebug`
  - 之后:`$G assembleDebug testDebugUnitTest --offline -q`
  - 均在 `clients/android/` 下执行。
- 本机无设备/AVD:真机验证由 hiro 安装 APK 完成,不得声称已在设备上验证。
- 工作区有未跟踪的 `clients/ios/`,**永远不要提交**;只按具体路径 `git add`。
- 提交流程:分支 `feat/push-android`,每 Task 一次提交,完成后 `--ff-only` 合回 dev,不推送。commit 结尾 `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`。

## File Structure(相对 `clients/android/app/src/main/java/ai/eclosion/gateterm/android/`)

```
clients/android/gradle/libs.versions.toml        jna、firebase、google-services、coroutines
clients/android/build.gradle.kts                 google-services 插件 apply false
clients/android/app/build.gradle.kts             FFI 构建任务、条件应用 google-services、依赖
clients/android/.gitignore                       app/google-services.json
connection/ServerConnection.kt、ConnectionStore.kt   cloudServerId
connection/CloudLink.kt(新)                      按 cloudServerId 找连接、记录 cloudServerId
wire/Messages.kt、wire/ControlJson.kt            HelloOk.cloudServerId
cloud/CloudForm.kt(新)                           表单校验(纯逻辑)
cloud/CloudFailure.kt(新)                        错误归类 → 文案资源(纯逻辑)
cloud/CloudAccountViewModel.kt(新)
ui/cloud/CloudAccountScreen.kt(新)
push/AgentPush.kt(新)                            解析 data、tag、文案选择(纯逻辑)
push/AgentNotifications.kt(新)                   渠道、显示、撤回
push/GateMessagingService.kt(新)
push/PushRegistrar.kt(新)                        取 FCM token 并登记
ui/connections/ConnectionViewModel.kt            AppScreen.CloudAccount、记录 cloudServerId、openFromNotification
ui/connections/ConnectionListScreen.kt           账号入口图标
GatetermApp.kt、MainActivity.kt、GatetermApplication.kt
res/drawable/ic_notification.xml(新)、res/values*/strings.xml、AndroidManifest.xml
test: connection/CloudLinkTest.kt、wire/FrameAndControlTest.kt、cloud/CloudFormTest.kt、cloud/CloudFailureTest.kt、push/AgentPushTest.kt
```

---

### Task 1: 构建接线(Rust FFI、Firebase、google-services)

**Files:**
- Modify: `clients/android/gradle/libs.versions.toml`, `clients/android/build.gradle.kts`, `clients/android/app/build.gradle.kts`, `clients/android/.gitignore`
- Local only(不入库): `clients/android/app/google-services.json`

**Interfaces:**
- Produces: 编译期可用的 Kotlin 包 `uniffi.gateterm_cloud_ffi`(`CloudAccount`、`CloudStatus`、`Purpose`、`CloudFfiException`);`com.google.firebase.messaging.*`;APK 内含 `lib/arm64-v8a/libgateterm_cloud_ffi.so`。

- [ ] **Step 1: 建分支、前置检查**

```bash
cd ~/online/eclosion/Gateterm && git checkout -b feat/push-android
command -v cargo-ndk && echo "${ANDROID_NDK_HOME:?未设置}" && ls crates/cloud-ffi scripts/build-android-ffi.sh
```

任一项缺失 → 报告 BLOCKED,不要自行安装。

- [ ] **Step 2: 版本目录 `gradle/libs.versions.toml`**

`[versions]` 加:

```toml
jna = "5.17.0"
firebaseBom = "35.0.2"
googleServices = "4.5.0"
coroutines = "1.10.2"
```

`[libraries]` 加:

```toml
jna = { group = "net.java.dev.jna", name = "jna", version.ref = "jna" }
firebase-bom = { group = "com.google.firebase", name = "firebase-bom", version.ref = "firebaseBom" }
firebase-messaging = { group = "com.google.firebase", name = "firebase-messaging" }
kotlinx-coroutines-android = { group = "org.jetbrains.kotlinx", name = "kotlinx-coroutines-android", version.ref = "coroutines" }
```

`[plugins]` 加:

```toml
google-services = { id = "com.google.gms.google-services", version.ref = "googleServices" }
```

- [ ] **Step 3: 根 `build.gradle.kts`** 的 plugins 块加 `alias(libs.plugins.google.services) apply false`。

- [ ] **Step 4: `app/build.gradle.kts`**

plugins 块之后加:

```kotlin
// google-services.json 不入库:有文件才启用推送插件。开源构建照常可用,只是没有推送。
if (file("google-services.json").exists()) {
    apply(plugin = libs.plugins.google.services.get().pluginId)
}

// Rust cloud-ffi:arm64 的 .so 与 UniFFI Kotlin 绑定都生成到 build/,不入库。
val repoRoot: File = rootProject.projectDir.parentFile.parentFile
val ffiJniDir = layout.buildDirectory.dir("generated/ffi/jniLibs")
val ffiKotlinDir = layout.buildDirectory.dir("generated/ffi/kotlin")
val buildCloudFfi by tasks.registering(Exec::class) {
    description = "构建 cloud-ffi(arm64-v8a)并生成 Kotlin 绑定"
    workingDir = repoRoot
    commandLine(
        "bash", "scripts/build-android-ffi.sh",
        ffiJniDir.get().asFile.absolutePath,
        ffiKotlinDir.get().asFile.absolutePath,
    )
    inputs.dir(File(repoRoot, "crates/cloud-ffi/src"))
    inputs.dir(File(repoRoot, "crates/cloud-client/src"))
    inputs.file(File(repoRoot, "scripts/build-android-ffi.sh"))
    outputs.dir(ffiJniDir)
    outputs.dir(ffiKotlinDir)
}
tasks.named("preBuild") { dependsOn(buildCloudFfi) }
```

`android { ... }` 内加:

```kotlin
    defaultConfig {
        // (保留原有字段)
        ndk { abiFilters += "arm64-v8a" }
    }
    sourceSets["main"].jniLibs.srcDir(ffiJniDir)
    sourceSets["main"].java.srcDir(ffiKotlinDir)
```

(`ndk { abiFilters }` 合并进现有 `defaultConfig`,不要新建第二个。)

`dependencies` 加:

```kotlin
    implementation(libs.jna) { artifact { type = "aar" } }
    implementation(platform(libs.firebase.bom))
    implementation(libs.firebase.messaging)
    implementation(libs.kotlinx.coroutines.android)
```

- [ ] **Step 5: `.gitignore` 与本地配置文件**

`clients/android/.gitignore` 追加 `app/google-services.json`。

```bash
cp ~/online/google-services.json clients/android/app/google-services.json
git -C ~/online/eclosion/Gateterm check-ignore clients/android/app/google-services.json
```

Expected: 打印该路径(已忽略)。

- [ ] **Step 6: 构建验证**

在 `clients/android/` 下用「首次拉新依赖」命令跑 `assembleDebug`(见 Global Constraints)。

```bash
unzip -l app/build/outputs/apk/debug/app-debug.apk | grep -E "libgateterm_cloud_ffi.so|libjnidispatch.so"
ls app/build/generated/ffi/kotlin/uniffi/gateterm_cloud_ffi/
```

Expected: APK 内有 `lib/arm64-v8a/libgateterm_cloud_ffi.so` 与 JNA 的 `lib/arm64-v8a/libjnidispatch.so`;绑定文件存在。再跑一次 `$G assembleDebug testDebugUnitTest --offline -q` 确认离线可用、现有单测全绿。

再验证「无 google-services.json 也能构建」:临时 `mv app/google-services.json /tmp/gs.json`,`$G assembleDebug --offline -q` 成功后 `mv /tmp/gs.json app/google-services.json` 放回。

- [ ] **Step 7: Commit**

```bash
git add clients/android/gradle/libs.versions.toml clients/android/build.gradle.kts clients/android/app/build.gradle.kts clients/android/.gitignore
git commit -m "build(android): 接入 cloud-ffi(Rust/UniFFI)与 Firebase Messaging

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: 连接记录 `cloudServerId`

**Files:**
- Create: `connection/CloudLink.kt`, `app/src/test/java/ai/eclosion/gateterm/android/connection/CloudLinkTest.kt`
- Modify: `wire/Messages.kt`, `wire/ControlJson.kt`, `connection/ServerConnection.kt`, `connection/ConnectionStore.kt`, `ui/connections/ConnectionViewModel.kt`, `app/src/test/.../wire/FrameAndControlTest.kt`

**Interfaces:**
- Produces:
  - `ServerMsg.HelloOk(proto: Int, os: String = "", cloudServerId: String? = null)`
  - `ServerConnection.cloudServerId: String? = null`(持久化键 `cloudServerId`)
  - `object CloudLink { fun find(items: List<ServerConnection>, cloudServerId: String): ServerConnection?; fun record(items: List<ServerConnection>, connectionId: String, cloudServerId: String?): List<ServerConnection>? }`(`record` 无变化时返回 null)

- [ ] **Step 1: 失败测试**

`FrameAndControlTest.kt` 追加:

```kotlin
    @Test
    fun helloOkCarriesCloudServerId() {
        val with = ControlJson.parse("""{"type":"hello-ok","proto":1,"os":"macos","cloud_server_id":"srv-1"}""".toByteArray())
        assertEquals(ServerMsg.HelloOk(1, "macos", "srv-1"), with)
        val without = ControlJson.parse("""{"type":"hello-ok","proto":1}""".toByteArray())
        assertEquals(ServerMsg.HelloOk(1, "", null), without)
    }
```

新建 `CloudLinkTest.kt`:

```kotlin
package ai.eclosion.gateterm.android.connection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CloudLinkTest {
    private fun conn(id: String, cloud: String?, lastUsed: Long?) =
        ServerConnection(id = id, name = id, url = "http://$id", token = "t", createdAt = 0, lastUsedAt = lastUsed, cloudServerId = cloud)

    @Test
    fun findPicksMostRecentlyUsedMatch() {
        val items = listOf(conn("a", "s1", 10), conn("b", "s1", 20), conn("c", "s2", 30))
        assertEquals("b", CloudLink.find(items, "s1")?.id)
        assertNull(CloudLink.find(items, "nope"))
    }

    @Test
    fun recordUpdatesOnlyWhenChanged() {
        val items = listOf(conn("a", null, null), conn("b", "s1", null))
        val updated = CloudLink.record(items, "a", "s9")!!
        assertEquals("s9", updated.first { it.id == "a" }.cloudServerId)
        assertNull(CloudLink.record(updated, "a", "s9"))
        // server 退出 Cloud 后不再回带:清空
        assertNull(CloudLink.record(items, "b", "s1"))
        assertEquals(null, CloudLink.record(items, "b", null)!!.first { it.id == "b" }.cloudServerId)
    }
}
```

Run: `$G testDebugUnitTest --offline -q`
Expected: 编译失败。

- [ ] **Step 2: 实现**

`Messages.kt`:`data class HelloOk(val proto: Int, val os: String = "", val cloudServerId: String? = null) : ServerMsg()`

`ControlJson.kt` 的 `"hello-ok"` 分支:

```kotlin
        "hello-ok" -> ServerMsg.HelloOk(
            obj.optInt("proto", PROTO_VERSION),
            obj.optString("os").takeUnless { it == "null" }.orEmpty(),
            obj.optString("cloud_server_id").takeUnless { it.isEmpty() || it == "null" },
        )
```

`ServerConnection.kt`:构造参数末尾加 `val cloudServerId: String? = null,`(注释:server 登录 Gateterm Cloud 后在握手里回带的 id;推送靠它找回这条连接)。

`ConnectionStore.kt`:`load()` 构造时加 `cloudServerId = obj.optString("cloudServerId").takeUnless { it.isEmpty() || it == "null" },`;`save()` 链上加 `.put("cloudServerId", item.cloudServerId ?: JSONObject.NULL)`。

`connection/CloudLink.kt`:

```kotlin
package ai.eclosion.gateterm.android.connection

/** 推送里的 server_id ↔ 本地连接。纯函数,便于单测。 */
object CloudLink {
    /** 同一台 server 可能存了多条连接(不同地址):取最近用过的那条。 */
    fun find(items: List<ServerConnection>, cloudServerId: String): ServerConnection? =
        items.filter { it.cloudServerId == cloudServerId }.maxByOrNull { it.lastUsedAt ?: 0L }

    /** 把握手拿到的 id 记到连接上;没有变化返回 null,调用方据此跳过持久化。 */
    fun record(items: List<ServerConnection>, connectionId: String, cloudServerId: String?): List<ServerConnection>? {
        val current = items.firstOrNull { it.id == connectionId } ?: return null
        if (current.cloudServerId == cloudServerId) return null
        return items.map { if (it.id == connectionId) it.copy(cloudServerId = cloudServerId) else it }
    }
}
```

`ConnectionViewModel.connect()` 的 `next.onControl` 中,把

```kotlin
                if (msg is ServerMsg.HelloOk) _state.update { it.copy(serverOs = msg.os) }
```

改为

```kotlin
                if (msg is ServerMsg.HelloOk) {
                    _state.update { it.copy(serverOs = msg.os) }
                    CloudLink.record(_state.value.connections, id, msg.cloudServerId)?.let { updated ->
                        persist(updated)
                        _state.update { it.copy(connections = updated) }
                    }
                }
```

- [ ] **Step 3: 运行并提交**

Run: `$G assembleDebug testDebugUnitTest --offline -q`
Expected: 全绿。

```bash
git add clients/android/app/src
git commit -m "feat(android): 连接记录握手回带的 cloud_server_id

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Cloud 账号界面

**Files:**
- Create: `cloud/CloudForm.kt`, `cloud/CloudFailure.kt`, `cloud/CloudAccountViewModel.kt`, `ui/cloud/CloudAccountScreen.kt`, `push/PushRegistrar.kt`, tests `cloud/CloudFormTest.kt`, `cloud/CloudFailureTest.kt`
- Modify: `ui/connections/ConnectionViewModel.kt`(`AppScreen.CloudAccount`、open/close)、`ui/connections/ConnectionListScreen.kt`(入口图标)、`GatetermApp.kt`、`MainActivity.kt`(第二个 ViewModel)、`res/values/strings.xml`、`res/values-en/strings.xml`

**Interfaces:**
- Consumes: `uniffi.gateterm_cloud_ffi.{CloudAccount, CloudStatus, Purpose, CloudFfiException}`
- Produces:
  - `enum class CloudAction { SendRegisterCode, Register, Login, SendResetCode, ResetPassword, Logout, SendDeleteCode, DeleteAccount }`(`keepsSecrets`:三个 Send* 为 true)
  - `data class CloudForm(url, email, password, code, confirmDelete)`;`sealed interface CloudJob`;`fun prepare(form, loggedIn: Boolean, action): Result<CloudJob>`,失败为 `CloudFormError(@StringRes msg)`
  - `sealed interface CloudFailure { NotLoggedIn; Unauthorized; RateLimited(retryAfter: Long?); Api(code); Unreachable; Local }`、`fun CloudFailure.messageRes(): Int`、`fun CloudFfiException.toFailure(): CloudFailure`
  - `object PushRegistrar { fun pushAvailable(ctx): Boolean; suspend fun registerCurrentToken(ctx, account: CloudAccount); fun onNewToken(ctx, token: String) }`
  - `class CloudAccountViewModel(app): AndroidViewModel`,`state: StateFlow<CloudUiState>`,`fun run(action)`、字段更新函数、`Factory`

- [ ] **Step 1: 失败测试**

`cloud/CloudFormTest.kt`(规则与 desktop `crates/desktop/src/settings/cloud.rs` 一致):

```kotlin
package ai.eclosion.gateterm.android.cloud

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class CloudFormTest {
    private val form = CloudForm(url = " https://gateterm.signal.top ", email = " A@Example.com ", password = "pw-pw-pw", code = " 012345 ")

    @Test
    fun registerJobIsNormalized() {
        assertEquals(
            CloudJob.Register("https://gateterm.signal.top", "a@example.com", "012345", "pw-pw-pw"),
            prepare(form, loggedIn = false, CloudAction.Register).getOrThrow(),
        )
    }

    @Test
    fun validationErrors() {
        assertTrue(prepare(form.copy(url = "gateterm.signal.top"), false, CloudAction.Login).isFailure)
        assertTrue(prepare(form.copy(code = "12a456"), false, CloudAction.Register).isFailure)
        assertTrue(prepare(form.copy(password = "short"), false, CloudAction.ResetPassword).isFailure)
        assertTrue(prepare(form.copy(email = "nope"), false, CloudAction.SendRegisterCode).isFailure)
    }

    @Test
    fun actionsRequireMatchingLoginState() {
        assertTrue(prepare(form, loggedIn = true, CloudAction.Login).isFailure)
        assertTrue(prepare(form, loggedIn = false, CloudAction.Logout).isFailure)
        assertEquals(CloudJob.Logout, prepare(form, loggedIn = true, CloudAction.Logout).getOrThrow())
    }

    @Test
    fun deleteRequiresConfirmation() {
        assertTrue(prepare(form, true, CloudAction.DeleteAccount).isFailure)
        assertEquals(
            CloudJob.DeleteAccount("pw-pw-pw", "012345"),
            prepare(form.copy(confirmDelete = true), true, CloudAction.DeleteAccount).getOrThrow(),
        )
    }

    @Test
    fun sendCodeKeepsSecrets() {
        assertTrue(CloudAction.SendRegisterCode.keepsSecrets)
        assertTrue(!CloudAction.Register.keepsSecrets)
    }
}
```

`cloud/CloudFailureTest.kt`:

```kotlin
package ai.eclosion.gateterm.android.cloud

import ai.eclosion.gateterm.android.R
import org.junit.Assert.assertEquals
import org.junit.Test

class CloudFailureTest {
    @Test
    fun apiCodesMapToSpecificMessages() {
        assertEquals(R.string.cloud_err_invalid_credentials, CloudFailure.Api("invalid_credentials").messageRes())
        assertEquals(R.string.cloud_err_invalid_code, CloudFailure.Api("invalid_code").messageRes())
        assertEquals(R.string.cloud_err_code_exhausted, CloudFailure.Api("code_exhausted").messageRes())
        assertEquals(R.string.cloud_err_email_taken, CloudFailure.Api("email_taken").messageRes())
        assertEquals(R.string.cloud_err_weak_password, CloudFailure.Api("weak_password").messageRes())
        assertEquals(R.string.cloud_err_wrong_password, CloudFailure.Api("wrong_password").messageRes())
        assertEquals(R.string.cloud_err_generic, CloudFailure.Api("something_new").messageRes())
    }

    @Test
    fun otherFailures() {
        assertEquals(R.string.cloud_err_unauthorized, CloudFailure.Unauthorized.messageRes())
        assertEquals(R.string.cloud_err_unreachable, CloudFailure.Unreachable.messageRes())
        assertEquals(R.string.cloud_err_rate_limited, CloudFailure.RateLimited(30).messageRes())
        assertEquals(R.string.cloud_err_not_logged_in, CloudFailure.NotLoggedIn.messageRes())
    }
}
```

Run: `$G testDebugUnitTest --offline -q` → 编译失败。

- [ ] **Step 2: 纯逻辑实现**

`cloud/CloudForm.kt`:

```kotlin
package ai.eclosion.gateterm.android.cloud

import ai.eclosion.gateterm.android.R
import androidx.annotation.StringRes

enum class CloudAction {
    SendRegisterCode, Register, Login, SendResetCode, ResetPassword, Logout, SendDeleteCode, DeleteAccount;

    /** 发码只是中间一步,用户接下来还要用刚填的密码 —— 这时不清空。 */
    val keepsSecrets: Boolean get() = this == SendRegisterCode || this == SendResetCode || this == SendDeleteCode
}

data class CloudForm(
    val url: String = "",
    val email: String = "",
    val password: String = "",
    val code: String = "",
    val confirmDelete: Boolean = false,
)

sealed interface CloudJob {
    data class RequestCode(val url: String, val email: String, val reset: Boolean) : CloudJob
    data class Register(val url: String, val email: String, val code: String, val password: String) : CloudJob
    data class Login(val url: String, val email: String, val password: String) : CloudJob
    data class ResetPassword(val url: String, val email: String, val code: String, val password: String) : CloudJob
    data object Logout : CloudJob
    data object RequestDeleteCode : CloudJob
    data class DeleteAccount(val password: String, val code: String) : CloudJob
}

class CloudFormError(@StringRes val msg: Int) : Exception()

private fun fail(@StringRes msg: Int): Nothing = throw CloudFormError(msg)

private fun CloudForm.urlOrFail(): String =
    url.trim().takeIf { it.startsWith("http://") || it.startsWith("https://") } ?: fail(R.string.cloud_form_bad_url)

private fun CloudForm.emailOrFail(): String = email.trim().lowercase().takeIf { '@' in it } ?: fail(R.string.cloud_form_bad_email)

private fun CloudForm.newPasswordOrFail(): String = password.takeIf { it.length in 8..128 } ?: fail(R.string.cloud_form_bad_password)

private fun CloudForm.codeOrFail(): String =
    code.trim().takeIf { it.length == 6 && it.all(Char::isDigit) } ?: fail(R.string.cloud_form_bad_code)

/** 与 desktop `settings::cloud::prepare` 同一套规则。 */
fun prepare(form: CloudForm, loggedIn: Boolean, action: CloudAction): Result<CloudJob> = runCatching {
    val needsLogin = action == CloudAction.Logout || action == CloudAction.SendDeleteCode || action == CloudAction.DeleteAccount
    if (needsLogin != loggedIn) fail(if (loggedIn) R.string.cloud_form_already_logged_in else R.string.cloud_form_not_logged_in)
    when (action) {
        CloudAction.SendRegisterCode -> CloudJob.RequestCode(form.urlOrFail(), form.emailOrFail(), reset = false)
        CloudAction.SendResetCode -> CloudJob.RequestCode(form.urlOrFail(), form.emailOrFail(), reset = true)
        CloudAction.Register -> CloudJob.Register(form.urlOrFail(), form.emailOrFail(), form.codeOrFail(), form.newPasswordOrFail())
        CloudAction.Login -> {
            if (form.password.isEmpty()) fail(R.string.cloud_form_need_password)
            CloudJob.Login(form.urlOrFail(), form.emailOrFail(), form.password)
        }
        CloudAction.ResetPassword -> CloudJob.ResetPassword(form.urlOrFail(), form.emailOrFail(), form.codeOrFail(), form.newPasswordOrFail())
        CloudAction.Logout -> CloudJob.Logout
        CloudAction.SendDeleteCode -> CloudJob.RequestDeleteCode
        CloudAction.DeleteAccount -> {
            if (!form.confirmDelete) fail(R.string.cloud_form_confirm_delete)
            if (form.password.isEmpty()) fail(R.string.cloud_form_need_password)
            CloudJob.DeleteAccount(form.password, form.codeOrFail())
        }
    }
}
```

`cloud/CloudFailure.kt`:

```kotlin
package ai.eclosion.gateterm.android.cloud

import ai.eclosion.gateterm.android.R
import androidx.annotation.StringRes
import uniffi.gateterm_cloud_ffi.CloudFfiException

/** 与 FFI 解耦的错误分类:单测不必加载 native 库。 */
sealed interface CloudFailure {
    data object NotLoggedIn : CloudFailure
    data object Unauthorized : CloudFailure
    data class RateLimited(val retryAfter: Long?) : CloudFailure
    data class Api(val code: String) : CloudFailure
    data object Unreachable : CloudFailure
    data object Local : CloudFailure
}

@StringRes
fun CloudFailure.messageRes(): Int = when (this) {
    CloudFailure.NotLoggedIn -> R.string.cloud_err_not_logged_in
    CloudFailure.Unauthorized -> R.string.cloud_err_unauthorized
    is CloudFailure.RateLimited -> R.string.cloud_err_rate_limited
    CloudFailure.Unreachable -> R.string.cloud_err_unreachable
    CloudFailure.Local -> R.string.cloud_err_local
    is CloudFailure.Api -> when (code) {
        "invalid_credentials" -> R.string.cloud_err_invalid_credentials
        "invalid_code" -> R.string.cloud_err_invalid_code
        "code_exhausted" -> R.string.cloud_err_code_exhausted
        "email_taken" -> R.string.cloud_err_email_taken
        "weak_password" -> R.string.cloud_err_weak_password
        "invalid_email" -> R.string.cloud_err_invalid_email
        "wrong_password" -> R.string.cloud_err_wrong_password
        else -> R.string.cloud_err_generic
    }
}

fun CloudFfiException.toFailure(): CloudFailure = when (this) {
    is CloudFfiException.NotLoggedIn -> CloudFailure.NotLoggedIn
    is CloudFfiException.Unauthorized -> CloudFailure.Unauthorized
    is CloudFfiException.RateLimited -> CloudFailure.RateLimited(retryAfter?.toLong())
    is CloudFfiException.Api -> CloudFailure.Api(code)
    is CloudFfiException.Unreachable -> CloudFailure.Unreachable
    is CloudFfiException.Local -> CloudFailure.Local
}
```

(生成的异常类名/字段名以 Task 1 生成的绑定为准;若 UniFFI 生成的是 `CloudFfiException.RateLimited(retryAfter: ULong?)`,`toLong()` 写法即正确。)

- [ ] **Step 3: 文案(两份 strings.xml,key 一一对应)**

中文 `res/values/strings.xml`:

```xml
    <string name="cloud_title">Cloud 账号</string>
    <string name="cloud_intro">可选。登录后,agent 在等你时会推送通知到这台手机。</string>
    <string name="cloud_url">Cloud 地址</string>
    <string name="cloud_email">邮箱</string>
    <string name="cloud_password">密码</string>
    <string name="cloud_code">验证码</string>
    <string name="cloud_code_hint">注册或重置密码时需要</string>
    <string name="cloud_login">登录</string>
    <string name="cloud_send_register_code">发送注册验证码</string>
    <string name="cloud_register">注册</string>
    <string name="cloud_send_reset_code">发送重置验证码</string>
    <string name="cloud_reset_password">用上面的密码重置</string>
    <string name="cloud_logged_in_as">已登录:%1$s</string>
    <string name="cloud_logout">退出登录</string>
    <string name="cloud_delete_title">删除账号</string>
    <string name="cloud_delete_hint">永久删除 Cloud 账号及其全部数据,需要密码和发到邮箱的确认码。</string>
    <string name="cloud_send_delete_code">发送确认码</string>
    <string name="cloud_confirm_delete">我明白删除后无法恢复</string>
    <string name="cloud_delete_account">永久删除账号</string>
    <string name="cloud_notifications_off">通知已关闭,收不到推送</string>
    <string name="cloud_open_notification_settings">去开启</string>
    <string name="cloud_push_unavailable">此构建未包含推送配置,登录后不会收到通知</string>
    <string name="cloud_ok_code_sent">验证码已发送到 %1$s,10 分钟内有效</string>
    <string name="cloud_ok_reset_code_sent">如果该邮箱已注册,验证码已发送</string>
    <string name="cloud_ok_registered">注册成功,已登录</string>
    <string name="cloud_ok_logged_in">已登录</string>
    <string name="cloud_ok_reset">密码已重置,所有设备都需要重新登录</string>
    <string name="cloud_ok_logged_out">已退出登录</string>
    <string name="cloud_ok_logged_out_local">已在本机退出登录(未能通知 Cloud)</string>
    <string name="cloud_ok_delete_code_sent">确认码已发送到 %1$s</string>
    <string name="cloud_ok_deleted">账号已删除</string>
    <string name="cloud_form_bad_url">Cloud 地址必须以 http:// 或 https:// 开头</string>
    <string name="cloud_form_bad_email">请填写邮箱</string>
    <string name="cloud_form_bad_password">密码长度需为 8–128 个字符</string>
    <string name="cloud_form_bad_code">请填写邮件里的 6 位验证码</string>
    <string name="cloud_form_need_password">请填写密码</string>
    <string name="cloud_form_confirm_delete">请先勾选「我明白删除后无法恢复」</string>
    <string name="cloud_form_already_logged_in">已登录;请先退出登录</string>
    <string name="cloud_form_not_logged_in">当前未登录</string>
    <string name="cloud_err_not_logged_in">当前未登录 Cloud</string>
    <string name="cloud_err_unauthorized">Cloud 登录已失效,请重新登录</string>
    <string name="cloud_err_rate_limited">操作太频繁,请稍后再试</string>
    <string name="cloud_err_unreachable">无法连接 Cloud</string>
    <string name="cloud_err_local">本地保存登录状态失败</string>
    <string name="cloud_err_invalid_credentials">邮箱或密码错误</string>
    <string name="cloud_err_invalid_code">验证码错误或已过期</string>
    <string name="cloud_err_code_exhausted">验证码错误次数过多,请重新获取</string>
    <string name="cloud_err_email_taken">该邮箱已注册</string>
    <string name="cloud_err_weak_password">密码长度需为 8–128 个字符</string>
    <string name="cloud_err_invalid_email">邮箱格式不正确</string>
    <string name="cloud_err_wrong_password">密码错误</string>
    <string name="cloud_err_generic">Cloud 返回了错误</string>
```

英文 `res/values-en/strings.xml`(同 key):

```xml
    <string name="cloud_title">Cloud account</string>
    <string name="cloud_intro">Optional. Once signed in, this phone gets a notification when an agent is waiting for you.</string>
    <string name="cloud_url">Cloud URL</string>
    <string name="cloud_email">Email</string>
    <string name="cloud_password">Password</string>
    <string name="cloud_code">Code</string>
    <string name="cloud_code_hint">Needed to sign up or reset the password</string>
    <string name="cloud_login">Sign in</string>
    <string name="cloud_send_register_code">Send sign-up code</string>
    <string name="cloud_register">Sign up</string>
    <string name="cloud_send_reset_code">Send reset code</string>
    <string name="cloud_reset_password">Reset to the password above</string>
    <string name="cloud_logged_in_as">Signed in as %1$s</string>
    <string name="cloud_logout">Sign out</string>
    <string name="cloud_delete_title">Delete account</string>
    <string name="cloud_delete_hint">Permanently deletes the Cloud account and all its data. Requires your password and a code sent by email.</string>
    <string name="cloud_send_delete_code">Send code</string>
    <string name="cloud_confirm_delete">I understand this cannot be undone</string>
    <string name="cloud_delete_account">Delete account permanently</string>
    <string name="cloud_notifications_off">Notifications are off, so pushes won\'t show</string>
    <string name="cloud_open_notification_settings">Turn on</string>
    <string name="cloud_push_unavailable">This build has no push configuration; you won\'t get notifications</string>
    <string name="cloud_ok_code_sent">Code sent to %1$s, valid for 10 minutes</string>
    <string name="cloud_ok_reset_code_sent">If that email is registered, a code has been sent</string>
    <string name="cloud_ok_registered">Signed up and signed in</string>
    <string name="cloud_ok_logged_in">Signed in</string>
    <string name="cloud_ok_reset">Password reset; every device must sign in again</string>
    <string name="cloud_ok_logged_out">Signed out</string>
    <string name="cloud_ok_logged_out_local">Signed out on this device (could not reach Cloud)</string>
    <string name="cloud_ok_delete_code_sent">Code sent to %1$s</string>
    <string name="cloud_ok_deleted">Account deleted</string>
    <string name="cloud_form_bad_url">The Cloud URL must start with http:// or https://</string>
    <string name="cloud_form_bad_email">Enter your email</string>
    <string name="cloud_form_bad_password">Password must be 8–128 characters</string>
    <string name="cloud_form_bad_code">Enter the 6-digit code from the email</string>
    <string name="cloud_form_need_password">Enter your password</string>
    <string name="cloud_form_confirm_delete">Tick "I understand this cannot be undone" first</string>
    <string name="cloud_form_already_logged_in">Already signed in; sign out first</string>
    <string name="cloud_form_not_logged_in">Not signed in</string>
    <string name="cloud_err_not_logged_in">Not signed in to Cloud</string>
    <string name="cloud_err_unauthorized">Your Cloud session expired; sign in again</string>
    <string name="cloud_err_rate_limited">Too many attempts; try again later</string>
    <string name="cloud_err_unreachable">Can\'t reach Cloud</string>
    <string name="cloud_err_local">Couldn\'t save the sign-in state on this device</string>
    <string name="cloud_err_invalid_credentials">Wrong email or password</string>
    <string name="cloud_err_invalid_code">The code is wrong or expired</string>
    <string name="cloud_err_code_exhausted">Too many wrong codes; request a new one</string>
    <string name="cloud_err_email_taken">That email is already registered</string>
    <string name="cloud_err_weak_password">Password must be 8–128 characters</string>
    <string name="cloud_err_invalid_email">That email address is invalid</string>
    <string name="cloud_err_wrong_password">Wrong password</string>
    <string name="cloud_err_generic">Cloud returned an error</string>
```

Run: `$G testDebugUnitTest --offline -q` → CloudForm/CloudFailure 测试 PASS。

- [ ] **Step 4: `push/PushRegistrar.kt`**

```kotlin
package ai.eclosion.gateterm.android.push

import android.content.Context
import android.util.Log
import com.google.firebase.FirebaseApp
import com.google.firebase.messaging.FirebaseMessaging
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import uniffi.gateterm_cloud_ffi.CloudAccount
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** 取 FCM token 并登记到 Cloud。没有 google-services.json 的构建里整体降级为空操作。 */
object PushRegistrar {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    fun pushAvailable(context: Context): Boolean = FirebaseApp.getApps(context).isNotEmpty()

    fun account(context: Context): CloudAccount =
        CloudAccount(java.io.File(context.filesDir, "cloud.json").absolutePath, android.os.Build.MODEL ?: "Android")

    suspend fun registerCurrentToken(context: Context, account: CloudAccount) {
        if (!pushAvailable(context)) return
        val token = suspendCancellableCoroutine { cont ->
            FirebaseMessaging.getInstance().token
                .addOnSuccessListener { cont.resume(it) }
                .addOnFailureListener { cont.resumeWithException(it) }
        }
        account.registerPushTarget("fcm", token)
    }

    /** FCM 换了 token:已登录就重新登记;失败只记日志,下次启动/登录会再登记。 */
    fun onNewToken(context: Context, token: String) {
        val app = context.applicationContext
        scope.launch {
            val account = account(app)
            if (account.status() == null) return@launch
            runCatching { account.registerPushTarget("fcm", token) }
                .onFailure { Log.w("Gateterm", "登记推送 token 失败", it) }
        }
    }

    /** App 启动时调用:已登录则补登一次(token 可能在 App 未运行时变过)。 */
    fun refreshOnStart(context: Context) {
        val app = context.applicationContext
        scope.launch {
            val account = account(app)
            if (account.status() == null) return@launch
            runCatching { registerCurrentToken(app, account) }
                .onFailure { Log.w("Gateterm", "登记推送 token 失败", it) }
        }
    }
}
```

- [ ] **Step 5: `cloud/CloudAccountViewModel.kt`**

```kotlin
package ai.eclosion.gateterm.android.cloud

import ai.eclosion.gateterm.android.R
import ai.eclosion.gateterm.android.push.PushRegistrar
import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelProvider.AndroidViewModelFactory.Companion.APPLICATION_KEY
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.gateterm_cloud_ffi.CloudFfiException
import uniffi.gateterm_cloud_ffi.CloudStatus
import uniffi.gateterm_cloud_ffi.Purpose

const val DEFAULT_CLOUD_URL = "https://gateterm.signal.top"

/** 一条给用户看的结果:成功文案或失败文案(资源 id + 可选参数)。 */
data class CloudMessage(val ok: Boolean, val res: Int, val arg: String? = null)

data class CloudUiState(
    val status: CloudStatus? = null,
    val form: CloudForm = CloudForm(url = DEFAULT_CLOUD_URL),
    val busy: Boolean = false,
    val message: CloudMessage? = null,
    /** 登录成功后置 true,界面据此申请通知权限,随后清零。 */
    val askNotificationPermission: Boolean = false,
)

class CloudAccountViewModel(private val app: Application) : AndroidViewModel(app) {
    private val account = PushRegistrar.account(app)
    private val _state = MutableStateFlow(CloudUiState(status = account.status()))
    val state: StateFlow<CloudUiState> = _state.asStateFlow()

    fun updateForm(transform: (CloudForm) -> CloudForm) = _state.update { it.copy(form = transform(it.form), message = null) }

    fun consumePermissionRequest() = _state.update { it.copy(askNotificationPermission = false) }

    fun run(action: CloudAction) {
        val st = _state.value
        if (st.busy) return
        val job = prepare(st.form, st.status != null, action).getOrElse { e ->
            val res = (e as? CloudFormError)?.msg ?: R.string.cloud_err_generic
            _state.update { it.copy(message = CloudMessage(false, res)) }
            return
        }
        _state.update { it.copy(busy = true, message = null) }
        viewModelScope.launch {
            val result = runCatching { withContext(Dispatchers.IO) { execute(job) } }
            val message = result.fold(
                onSuccess = { it },
                onFailure = { e -> CloudMessage(false, (e as? CloudFfiException)?.toFailure()?.messageRes() ?: R.string.cloud_err_generic) },
            )
            val loggedInNow = result.isSuccess && (job is CloudJob.Login || job is CloudJob.Register)
            _state.update {
                it.copy(
                    busy = false,
                    status = account.status(),
                    message = message,
                    form = if (result.isSuccess && !action.keepsSecrets) it.form.copy(password = "", code = "", confirmDelete = false) else it.form,
                    askNotificationPermission = loggedInNow,
                )
            }
        }
    }

    private suspend fun execute(job: CloudJob): CloudMessage = when (job) {
        is CloudJob.RequestCode -> {
            account.requestCode(job.url, job.email, if (job.reset) Purpose.RESET else Purpose.REGISTER)
            if (job.reset) CloudMessage(true, R.string.cloud_ok_reset_code_sent) else CloudMessage(true, R.string.cloud_ok_code_sent, job.email)
        }
        is CloudJob.Register -> {
            account.register(job.url, job.email, job.code, job.password)
            runCatching { PushRegistrar.registerCurrentToken(app, account) }
            CloudMessage(true, R.string.cloud_ok_registered)
        }
        is CloudJob.Login -> {
            account.login(job.url, job.email, job.password)
            runCatching { PushRegistrar.registerCurrentToken(app, account) }
            CloudMessage(true, R.string.cloud_ok_logged_in)
        }
        is CloudJob.ResetPassword -> {
            account.resetPassword(job.url, job.email, job.code, job.password)
            CloudMessage(true, R.string.cloud_ok_reset)
        }
        CloudJob.Logout -> if (account.logout()) CloudMessage(true, R.string.cloud_ok_logged_out) else CloudMessage(true, R.string.cloud_ok_logged_out_local)
        CloudJob.RequestDeleteCode -> CloudMessage(true, R.string.cloud_ok_delete_code_sent, account.requestDeleteCode())
        is CloudJob.DeleteAccount -> {
            account.deleteAccount(job.password, job.code)
            CloudMessage(true, R.string.cloud_ok_deleted)
        }
    }

    companion object {
        val Factory: ViewModelProvider.Factory = viewModelFactory {
            initializer { CloudAccountViewModel(checkNotNull(this[APPLICATION_KEY])) }
        }
    }
}
```

(UniFFI 生成的枚举常量名以绑定为准,通常是 `Purpose.REGISTER` / `Purpose.RESET`。)

- [ ] **Step 6: `ui/cloud/CloudAccountScreen.kt`**

无状态 Composable,参数:`state: CloudUiState`、`pushAvailable: Boolean`、`notificationsEnabled: Boolean`、`onForm: ((CloudForm) -> CloudForm) -> Unit`、`onAction: (CloudAction) -> Unit`、`onOpenNotificationSettings: () -> Unit`、`onBack: () -> Unit`。布局沿用 `AppearanceScreen` 的 `Scaffold + TopAppBar(返回) + 可滚动 Column` 写法:

- 顶部 `cloud_intro`;`!pushAvailable` 时显示 `cloud_push_unavailable`;已登录且 `!notificationsEnabled` 时显示 `cloud_notifications_off` 与按钮 `cloud_open_notification_settings`。
- 未登录:`OutlinedTextField` × 4(url、email、password[`PasswordVisualTransformation`]、code + supportingText `cloud_code_hint`);按钮行 1:登录 / 发送注册验证码 / 注册;行 2:发送重置验证码 / 用上面的密码重置。
- 已登录:`cloud_logged_in_as`(email)与 url 小字;「退出登录」;可展开的「删除账号」区(密码、确认码 + 发送确认码、勾选框、永久删除按钮,删除按钮用 error 色)。
- `state.busy` 时所有按钮 `enabled = false`,标题旁 `CircularProgressIndicator`。
- `state.message`:成功用 primary 色,失败用 error 色,`stringResource(res, arg)`(arg 为 null 时不传)。

- [ ] **Step 7: 接入导航**

- `ConnectionViewModel.kt`:`sealed class AppScreen` 加 `data object CloudAccount : AppScreen()`;加 `fun openCloudAccount()` / `fun closeCloudAccount()`(照 `openAppearance`/`closeAppearance`)。
- `ConnectionListScreen.kt`:参数加 `onCloudAccount: () -> Unit`;`TopAppBar.actions` 在设置图标前加 `IconButton(onClick = onCloudAccount) { Icon(Icons.Filled.AccountCircle, contentDescription = stringResource(R.string.cloud_title)) }`。
- `MainActivity.kt`:加 `private val cloudViewModel: CloudAccountViewModel by viewModels { CloudAccountViewModel.Factory }`,把它传给 `GatetermApp(connectionViewModel, cloudViewModel)`。
- `GatetermApp.kt`:签名加 `cloudViewModel: CloudAccountViewModel`;`AppScreen.List` 分支传 `onCloudAccount = viewModel::openCloudAccount`;新增分支:

```kotlin
            AppScreen.CloudAccount -> {
                BackHandler(onBack = viewModel::closeCloudAccount)
                val cloud by cloudViewModel.state.collectAsStateWithLifecycle()
                val notificationsEnabled = remember(cloud.status, cloud.message) {
                    NotificationManagerCompat.from(context).areNotificationsEnabled()
                }
                val permissionLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }
                LaunchedEffect(cloud.askNotificationPermission) {
                    if (cloud.askNotificationPermission) {
                        cloudViewModel.consumePermissionRequest()
                        if (Build.VERSION.SDK_INT >= 33) permissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                    }
                }
                CloudAccountScreen(
                    state = cloud,
                    pushAvailable = remember { PushRegistrar.pushAvailable(context) },
                    notificationsEnabled = notificationsEnabled,
                    onForm = cloudViewModel::updateForm,
                    onAction = cloudViewModel::run,
                    onOpenNotificationSettings = {
                        context.startActivity(
                            Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName),
                        )
                    },
                    onBack = viewModel::closeCloudAccount,
                )
            }
```

`AndroidManifest.xml` 加 `<uses-permission android:name="android.permission.POST_NOTIFICATIONS" />`。

- [ ] **Step 8: 构建、测试、提交**

Run: `$G assembleDebug testDebugUnitTest --offline -q`
Expected: 全绿。

```bash
git add clients/android/app/src
git commit -m "feat(android): Cloud 账号界面(UniFFI 复用 Rust 账号逻辑)

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: 接收推送并显示/撤回通知

**Files:**
- Create: `push/AgentPush.kt`, `push/AgentNotifications.kt`, `push/GateMessagingService.kt`, `res/drawable/ic_notification.xml`, test `push/AgentPushTest.kt`
- Modify: `GatetermApplication.kt`, `AndroidManifest.xml`, 两份 `strings.xml`

**Interfaces:**
- Produces:
  - `data class AgentPush(serverId, session: Long, agentId, kind: Kind, toolName: String?, expiresAt: Long?)`,`enum class Kind { Waiting, Permission, Resolved }`;`companion fun parse(data: Map<String, String>): AgentPush?`(`done`/未知 kind/缺字段 → null);`val tag`;`val notificationId: Int`;`fun timeoutMillis(nowSeconds: Long): Long?`
  - `fun agentDisplayName(agentId: String): String`
  - `object AgentNotifications { const val CHANNEL; const val EXTRA_SERVER_ID; const val EXTRA_SESSION; fun ensureChannel(ctx); fun handle(ctx, push) }`

- [ ] **Step 1: 失败测试 `push/AgentPushTest.kt`**

```kotlin
package ai.eclosion.gateterm.android.push

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AgentPushTest {
    private val base = mapOf("server_id" to "srv", "session" to "7", "agent_id" to "claude-code", "kind" to "waiting")

    @Test
    fun parsesWaiting() {
        val p = AgentPush.parse(base)!!
        assertEquals(AgentPush("srv", 7, "claude-code", AgentPush.Kind.Waiting, null, null), p)
        assertEquals("srv:7:claude-code", p.tag)
        assertEquals("srv:7:claude-code".hashCode(), p.notificationId)
    }

    @Test
    fun parsesPermissionWithToolAndExpiry() {
        val p = AgentPush.parse(base + mapOf("kind" to "permission", "tool_name" to "Bash", "expires_at" to "1000"))!!
        assertEquals(AgentPush.Kind.Permission, p.kind)
        assertEquals("Bash", p.toolName)
        assertEquals(400_000L, p.timeoutMillis(nowSeconds = 600))
        assertEquals(0L, p.timeoutMillis(nowSeconds = 2000))
        assertNull(AgentPush.parse(base)!!.timeoutMillis(600))
    }

    @Test
    fun ignoresDoneUnknownAndBroken() {
        assertNull(AgentPush.parse(base + ("kind" to "done")))
        assertNull(AgentPush.parse(base + ("kind" to "boom")))
        assertNull(AgentPush.parse(base - "server_id"))
        assertNull(AgentPush.parse(base + ("session" to "x")))
        assertEquals(AgentPush.Kind.Resolved, AgentPush.parse(base + ("kind" to "resolved"))!!.kind)
    }

    @Test
    fun displayNames() {
        assertEquals("Claude Code", agentDisplayName("claude-code"))
        assertEquals("Codex", agentDisplayName("codex"))
        assertEquals("some-new-agent", agentDisplayName("some-new-agent"))
    }
}
```

Run → 编译失败。

- [ ] **Step 2: `push/AgentPush.kt`**

```kotlin
package ai.eclosion.gateterm.android.push

/** FCM 数据消息 → 结构化推送。只有路由所需的最小信息;文案在本地生成。 */
data class AgentPush(
    val serverId: String,
    val session: Long,
    val agentId: String,
    val kind: Kind,
    val toolName: String?,
    val expiresAt: Long?,
) {
    enum class Kind { Waiting, Permission, Resolved }

    /** 同一会话里同一个 agent 只占一条通知:新的覆盖旧的,resolved 按它撤回。
     *  带上 agent_id:一个终端会话里跑着两个 agent 时,一个的撤回不该误伤另一个。 */
    val tag: String get() = "$serverId:$session:$agentId"
    val notificationId: Int get() = tag.hashCode()

    /** 到期自动消失的剩余毫秒;没有过期时间返回 null,已过期返回 0。 */
    fun timeoutMillis(nowSeconds: Long): Long? = expiresAt?.let { ((it - nowSeconds) * 1000).coerceAtLeast(0) }

    companion object {
        fun parse(data: Map<String, String>): AgentPush? {
            val serverId = data["server_id"]?.takeIf { it.isNotEmpty() } ?: return null
            val session = data["session"]?.toLongOrNull() ?: return null
            val agentId = data["agent_id"]?.takeIf { it.isNotEmpty() } ?: return null
            val kind = when (data["kind"]) {
                "waiting" -> Kind.Waiting
                "permission" -> Kind.Permission
                "resolved" -> Kind.Resolved
                else -> return null
            }
            return AgentPush(serverId, session, agentId, kind, data["tool_name"], data["expires_at"]?.toLongOrNull())
        }
    }
}

fun agentDisplayName(agentId: String): String = when (agentId) {
    "claude-code" -> "Claude Code"
    "codex" -> "Codex"
    "grok" -> "Grok"
    "pi" -> "Pi"
    "antigravity" -> "Antigravity"
    else -> agentId
}
```

- [ ] **Step 3: 文案、图标、通知实现**

strings(两份):

```xml
    <!-- values/ -->
    <string name="notif_channel_name">Agent 等待提醒</string>
    <string name="notif_waiting_title">%1$s 在等你</string>
    <string name="notif_permission_title">%1$s 请求授权</string>
    <string name="notif_permission_body">工具:%1$s</string>
    <string name="notif_tap_to_open">点按打开对应会话</string>
    <!-- values-en/ -->
    <string name="notif_channel_name">Agent waiting alerts</string>
    <string name="notif_waiting_title">%1$s is waiting for you</string>
    <string name="notif_permission_title">%1$s needs your approval</string>
    <string name="notif_permission_body">Tool: %1$s</string>
    <string name="notif_tap_to_open">Tap to open the session</string>
```

`res/drawable/ic_notification.xml`(单色,状态栏用):

```xml
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="24dp" android:height="24dp" android:viewportWidth="24" android:viewportHeight="24">
    <path android:fillColor="#FFFFFFFF"
        android:pathData="M3,4h18a1,1 0,0 1,1 1v14a1,1 0,0 1,-1 1H3a1,1 0,0 1,-1 -1V5a1,1 0,0 1,1 -1zM6.5,9.5l3,2.5 -3,2.5 1,1.2 4.4,-3.7 -4.4,-3.7zM12,15h6v1.5h-6z" />
</vector>
```

`push/AgentNotifications.kt`:

```kotlin
package ai.eclosion.gateterm.android.push

import ai.eclosion.gateterm.android.MainActivity
import ai.eclosion.gateterm.android.R
import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat

object AgentNotifications {
    const val CHANNEL = "agent_waiting"
    const val EXTRA_SERVER_ID = "gateterm.server_id"
    const val EXTRA_SESSION = "gateterm.session"

    fun ensureChannel(context: Context) {
        if (Build.VERSION.SDK_INT < 26) return
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.notif_channel_name), NotificationManager.IMPORTANCE_HIGH),
        )
    }

    fun handle(context: Context, push: AgentPush) {
        val nm = NotificationManagerCompat.from(context)
        if (push.kind == AgentPush.Kind.Resolved) {
            nm.cancel(push.tag, push.notificationId)
            return
        }
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) return

        val name = agentDisplayName(push.agentId)
        val title = when (push.kind) {
            AgentPush.Kind.Permission -> context.getString(R.string.notif_permission_title, name)
            else -> context.getString(R.string.notif_waiting_title, name)
        }
        val body = push.toolName?.takeIf { push.kind == AgentPush.Kind.Permission }
            ?.let { context.getString(R.string.notif_permission_body, it) }
            ?: context.getString(R.string.notif_tap_to_open)
        val open = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            .putExtra(EXTRA_SERVER_ID, push.serverId)
            .putExtra(EXTRA_SESSION, push.session)
        val pending = PendingIntent.getActivity(
            context, push.notificationId, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val builder = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(body)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setAutoCancel(true)
            .setContentIntent(pending)
        push.timeoutMillis(System.currentTimeMillis() / 1000)?.let { builder.setTimeoutAfter(it) }
        nm.notify(push.tag, push.notificationId, builder.build())
    }
}
```

`push/GateMessagingService.kt`:

```kotlin
package ai.eclosion.gateterm.android.push

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

class GateMessagingService : FirebaseMessagingService() {
    override fun onNewToken(token: String) = PushRegistrar.onNewToken(applicationContext, token)

    override fun onMessageReceived(message: RemoteMessage) {
        val push = AgentPush.parse(message.data) ?: return
        AgentNotifications.handle(applicationContext, push)
    }
}
```

`GatetermApplication.onCreate()` 末尾加:

```kotlin
        AgentNotifications.ensureChannel(this)
        PushRegistrar.refreshOnStart(this)
```

`AndroidManifest.xml` 的 `<application>` 内加:

```xml
        <service
            android:name=".push.GateMessagingService"
            android:exported="false">
            <intent-filter>
                <action android:name="com.google.firebase.MESSAGING_EVENT" />
            </intent-filter>
        </service>
```

- [ ] **Step 4: 构建、测试、提交**

Run: `$G assembleDebug testDebugUnitTest --offline -q`

```bash
git add clients/android/app/src
git commit -m "feat(android): 接收 FCM 推送,显示/撤回 agent 等待通知

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: 点通知进入对应会话

**Files:**
- Modify: `MainActivity.kt`, `AndroidManifest.xml`, `ui/connections/ConnectionViewModel.kt`, `GatetermApp.kt`(消息文案), 两份 `strings.xml`

**Interfaces:**
- Consumes: `CloudLink.find`、`AgentNotifications.EXTRA_*`
- Produces: `ConnectionViewModel.openFromNotification(cloudServerId: String, session: Long)`;`UserMessage.ConnectServerFirst`、`UserMessage.SessionGone`

- [ ] **Step 1: Manifest 与 Activity**

`MainActivity` 的 `<activity>` 加 `android:launchMode="singleTop"`。

`MainActivity.kt`:`onCreate` 末尾加 `if (savedInstanceState == null) handleIntent(intent)`;并加:

```kotlin
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        val serverId = intent?.getStringExtra(AgentNotifications.EXTRA_SERVER_ID) ?: return
        val session = intent.getLongExtra(AgentNotifications.EXTRA_SESSION, -1L).takeIf { it >= 0 } ?: return
        // 只处理一次:配置变更重建时不要重复跳转
        intent.removeExtra(AgentNotifications.EXTRA_SERVER_ID)
        connectionViewModel.openFromNotification(serverId, session)
    }
```

- [ ] **Step 2: ViewModel**

`UserMessage` 加 `data object ConnectServerFirst : UserMessage()`、`data object SessionGone : UserMessage()`;`GatetermApp.kt` 的 `messageText` 补两个分支,strings:

```xml
    <!-- values/ -->
    <string name="msg_connect_server_first">请先连接一次这台 server,之后点通知即可直达</string>
    <string name="msg_session_gone">该会话已经结束</string>
    <!-- values-en/ -->
    <string name="msg_connect_server_first">Connect to this server once first; after that, tapping a notification goes straight there</string>
    <string name="msg_session_gone">That session has ended</string>
```

`ConnectionViewModel` 加字段与方法:

```kotlin
    /** 点通知要去的 (连接 id, 会话 id);连上并拿到会话列表后消费。 */
    private var pendingOpen: Pair<String, Long>? = null

    fun openFromNotification(cloudServerId: String, session: Long) {
        val conn = CloudLink.find(_state.value.connections, cloudServerId) ?: run {
            _messages.tryEmit(UserMessage.ConnectServerFirst)
            return
        }
        pendingOpen = conn.id to session
        val screen = _state.value.screen
        if (screen is AppScreen.Server && screen.connectionId == conn.id && _state.value.connectingId == null) {
            client?.send(ClientMsg.ListSessions) // 列表回来后在 handleServer 里打开
            return
        }
        if (screen is AppScreen.Server) disconnect()
        connect(conn.id)
    }
```

`handleServer` 的 `is ServerMsg.Sessions` 分支改为:

```kotlin
            is ServerMsg.Sessions -> {
                _state.update { it.copy(sessions = msg.sessions) }
                consumePendingOpen()
            }
```

并加:

```kotlin
    private fun consumePendingOpen() {
        val (connId, session) = pendingOpen ?: return
        val screen = _state.value.screen
        if (screen !is AppScreen.Server || screen.connectionId != connId) return
        pendingOpen = null
        if (_state.value.sessions.any { it.id == session }) openSession(session) else _messages.tryEmit(UserMessage.SessionGone)
    }
```

(`SessionInfo` 的 id 字段名与类型以 `wire/Messages.kt` 为准;`openSession` 的参数类型同步。若 `connect()` 因 `connectingId != null` 提前返回,`pendingOpen` 会在那次连接成功后的 `Sessions` 里被消费 —— 这是期望行为。)

- [ ] **Step 3: 构建、测试、提交**

Run: `$G assembleDebug testDebugUnitTest --offline -q`

```bash
git add clients/android/app/src
git commit -m "feat(android): 点推送通知连上对应 server 并打开该会话

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: 交付 APK 与真机验证清单

**Files:** 无代码改动(验证有问题时回到对应 Task 修)。

- [ ] **Step 1: 产出 APK**

```bash
cd clients/android && $G assembleDebug --offline -q && ls -la app/build/outputs/apk/debug/app-debug.apk
```

- [ ] **Step 2: 交给 hiro 的真机清单**(前提:Cloud 已部署到 `https://gateterm.signal.top` 且配置了 FCM;本机 gateterm-server/desktop 已用同一账号登录 Cloud)

1. 安装 APK;连接一次自己的 server(使 `cloudServerId` 被记录)。
2. 首页右上角账号图标 → 登录(或注册)→ 允许通知权限。
3. 在电脑上让 Claude Code 进入等待(如请求执行一个需要授权的命令)。
4. 手机收到「Claude Code 请求授权 / 工具:Bash」通知。
5. 在电脑上处理掉 → 手机通知自动消失。
6. 再触发一次,点通知 → App 连上该 server 并打开对应会话终端。
7. 在系统设置关闭通知 → 账号页显示「通知已关闭」与「去开启」。
8. 退出登录 → 再触发等待 → 手机不再收到。

- [ ] **Step 3: 合回 dev(由 controller 在终审后执行)**

```bash
git checkout dev && git merge --ff-only feat/push-android
```
