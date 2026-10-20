package ai.eclosion.octoterm.android.push

/** Registration with Cloud is separate from signing in and Android's notification permission. */
enum class PushRegistration {
    Idle, Registering, Registered, MissingConfiguration, MissingPlayServices, TimedOut, NetworkError, CloudError, Failed,
}
