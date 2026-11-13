package ai.eclosion.gateterm.android.push

internal fun initializeFcmWithConsent(
    consentAccepted: Boolean,
    selectedProvider: String?,
    initialize: () -> Boolean,
): Boolean = consentAccepted && selectedProvider == "fcm" && initialize()
