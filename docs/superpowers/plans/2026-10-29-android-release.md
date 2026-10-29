# Android release workflow implementation plan

**Goal:** Include a signed arm64-v8a Android APK in the existing release-tag workflow using the four configured Repository Secrets.

**Architecture:** A separate Ubuntu job installs Java 17, Android SDK 36, build-tools 36.0.0, NDK r29 and Rust Android tooling. Gradle builds an unsigned release with the existing FFI task; a final signing step decodes the keystore in runner temporary storage, signs via apksigner environment-password arguments, verifies the APK and produces a SHA-256 file. The existing release job waits for this job and downloads its artifact.

**Constraints:** Use `ANDROID_KEYSTORE_BASE64`, `ANDROID_STORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` from Repository Secrets. No Environment configuration, committed keystores, password arguments or signing files in caches/artifacts. Keep existing release targets and manual build behavior. Preserve unrelated README edits. Following the user's request to include push configuration, optionally restore `ANDROID_GOOGLE_SERVICES_JSON` and `ANDROID_ALIYUN_SERVICES_JSON` as complete JSON client configuration files; reject mismatched package names and clean up after building.

- [x] Add Android/version-name validation to the existing tag version check.
- [x] Add the Android toolchain/build/sign/package job and its release dependency.
- [x] Restore optional Firebase/EMAS JSON Repository Secrets, validate `ai.eclosion.gateterm.android` and required EMAS fields before writing files, reject Admin service-account keys, and remove the files after building.
- [x] Validate workflow with actionlint; exercise the actual signing step against a locally built APK with the new keystore, including missing-secret failure and temporary-key cleanup.
- [x] Review the final diff and report validation limits. Commit, push and a new release tag are outside this change unless requested.

**Review focus:** Missing/invalid signing secrets must fail; signing reads passwords only from environment variables; Android build outputs stay unsigned until the signing step; APK filenames use the prepared release version; manual workflow runs upload artifacts without publishing; release publication waits for Android success.

**Validation:** actionlint v1.7.12 passed. Local full assembleRelease (including Rust FFI) succeeded. An init script produced a genuinely unsigned APK without changing local signing files; the workflow signing shell block then passed missing-secret/invalid-Base64/wrong-password cases, signed with the configured new key, verified its certificate fingerprint, 16KB zip alignment, SHA-256 checksum and temporary-key removal. The actual version shell block accepted release-v0.2.7, rejected a mismatched tag and supported a manual branch run. Independent review identified the pre-existing manual-tag-dispatch publishing edge case; publication now also requires a push event. Local validation used installed NDK 30 / build-tools 36.1; the Ubuntu job pins NDK 29 / build-tools 36.0. A GitHub-hosted run has not been triggered.

**Push configuration validation:** Nine cases passed for absent providers, each provider separately, both together, invalid JSON, Admin keys, malformed clients, old EMAS package names with no partial writes, and missing EMAS credentials. The local Firebase file matches Gateterm. The original EMAS file targeted `ai.eclosion.octoterm.android`; the user supplied D:/Downloads/aliyun-emas-services.json, which was validated for the current package and copied into the local app directory, preserving an external backup of the old configuration. Both JSON files remain ignored by Git and must be supplied to CI via the configured Secrets.

**Remote validation:** The user authorized committing/pushing the workflow and testing Release packaging with workflow_dispatch. Local actionlint, push-configuration scenarios and the real APK signing/checksum/cleanup checks passed again before committing. SSH authenticates as the repository owner; the available HTTP credential belongs to a different account without write access, so owner authorization for dispatch must be established separately.

**Dispatch adjustment:** Owner HTTP authorization and the browser tool were unavailable. Changes to `.github/workflows/release.yml` on `dev` now automatically run packaging validation through the owner's authorized Git push. Branch runs upload artifacts without publishing a Release; tag publication remains guarded by a tag push. GitHub ignores path filters for tag pushes, so existing release tags retain their behavior.
