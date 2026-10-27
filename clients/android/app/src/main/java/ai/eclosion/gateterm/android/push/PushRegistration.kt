package ai.eclosion.gateterm.android.push

/** Registration with Cloud is separate from signing in and Android's notification permission. */
enum class PushRegistration {
    Idle, Registering, Registered, RegisteredAliyun, MissingConfiguration, MissingPlayServices, TimedOut, NetworkError, CloudError, Failed,
    Disabled, ProviderMismatch, UnsupportedProvider;

    val registered: Boolean get() = this == Registered || this == RegisteredAliyun
}
