package ai.eclosion.gateterm.android.connection

enum class ServerConnectionStatus {
    Disconnected, Connecting, Connected, Reconnecting, Failed;
    val active: Boolean get() = this == Connecting || this == Connected || this == Reconnecting
}
