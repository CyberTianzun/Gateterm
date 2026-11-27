//! Ed25519 signing and verification for `update-manifest.json`.

use anyhow::Context;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;

/// Compile-time Ed25519 public key (base64-encoded 32 raw bytes).
///
/// Empty until the maintainer runs `manifest-tool generate-key`; while unconfigured,
/// verification always fails with `VerifyError::UnconfiguredPublicKey`.
pub const EMBEDDED_PUBLIC_KEY_B64: &str = "";

/// Failure modes while verifying a manifest signature.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    #[error("updater public key is not configured")]
    UnconfiguredPublicKey,
    #[error("invalid updater public key")]
    InvalidPublicKey,
    #[error("invalid updater signature encoding")]
    InvalidSignatureEncoding,
    #[error("manifest signature verification failed")]
    SignatureMismatch,
}

/// Returns the compile-time configured public key, preferring `GATETERM_UPDATER_PUBLIC_KEY`
/// if non-empty at compile time and otherwise falling back to `EMBEDDED_PUBLIC_KEY_B64`.
pub fn compiled_public_key_b64() -> &'static str {
    match option_env!("GATETERM_UPDATER_PUBLIC_KEY") {
        Some(key) if !key.trim().is_empty() => key,
        _ => EMBEDDED_PUBLIC_KEY_B64,
    }
}

/// Generates a fresh Ed25519 keypair encoded as `(private_b64, public_b64)`.
pub fn generate_keypair() -> (String, String) {
    let signing_key = SigningKey::generate(&mut OsRng);
    let private_b64 = STANDARD.encode(signing_key.to_bytes());
    let public_b64 = STANDARD.encode(signing_key.verifying_key().to_bytes());
    (private_b64, public_b64)
}

/// Signs raw manifest bytes using a base64-encoded 32-byte Ed25519 private key.
pub fn sign_manifest_bytes(bytes: &[u8], private_key_b64: &str) -> anyhow::Result<String> {
    let raw = STANDARD
        .decode(private_key_b64.trim())
        .context("invalid base64 private key")?;
    let key_bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| anyhow::anyhow!("Ed25519 private key must be 32 bytes"))?;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let signature = signing_key.sign(bytes);
    Ok(STANDARD.encode(signature.to_bytes()))
}

/// Verifies `sig_b64` against raw `bytes` using `public_key_b64`.
pub fn verify_manifest_bytes(
    bytes: &[u8],
    sig_b64: &str,
    public_key_b64: &str,
) -> Result<(), VerifyError> {
    let trimmed_key = public_key_b64.trim();
    if trimmed_key.is_empty() {
        return Err(VerifyError::UnconfiguredPublicKey);
    }

    let key_raw = STANDARD
        .decode(trimmed_key)
        .map_err(|_| VerifyError::InvalidPublicKey)?;
    let key_bytes: [u8; 32] = key_raw
        .try_into()
        .map_err(|_| VerifyError::InvalidPublicKey)?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_bytes).map_err(|_| VerifyError::InvalidPublicKey)?;

    let sig_raw = STANDARD
        .decode(sig_b64.trim())
        .map_err(|_| VerifyError::InvalidSignatureEncoding)?;
    let signature =
        Signature::from_slice(&sig_raw).map_err(|_| VerifyError::InvalidSignatureEncoding)?;

    verifying_key
        .verify_strict(bytes, &signature)
        .map_err(|_| VerifyError::SignatureMismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_roundtrip_and_rejects_tampering_wrong_key_empty_key_and_bad_base64() {
        let (priv_b64, pub_b64) = generate_keypair();
        let (_, other_pub) = generate_keypair();
        assert_ne!(pub_b64, other_pub);

        let payload = br#"{"schema_version":1}"#;
        let sig = sign_manifest_bytes(payload, &priv_b64).unwrap();
        assert!(verify_manifest_bytes(payload, &sig, &pub_b64).is_ok());
        assert!(verify_manifest_bytes(payload, &format!("{sig}\n"), &pub_b64).is_ok());
        assert!(matches!(
            verify_manifest_bytes(br#"{"schema_version":2}"#, &sig, &pub_b64),
            Err(VerifyError::SignatureMismatch)
        ));
        assert!(matches!(
            verify_manifest_bytes(payload, &sig, &other_pub),
            Err(VerifyError::SignatureMismatch)
        ));
        assert!(matches!(
            verify_manifest_bytes(payload, &sig, ""),
            Err(VerifyError::UnconfiguredPublicKey)
        ));
        assert!(matches!(
            verify_manifest_bytes(payload, &sig, "   "),
            Err(VerifyError::UnconfiguredPublicKey)
        ));
        assert!(matches!(
            verify_manifest_bytes(payload, "!!!not-b64!!!", &pub_b64),
            Err(VerifyError::InvalidSignatureEncoding)
        ));
        assert!(matches!(
            verify_manifest_bytes(payload, &STANDARD.encode([0u8; 32]), &pub_b64),
            Err(VerifyError::InvalidSignatureEncoding)
        ));
        assert!(matches!(
            verify_manifest_bytes(payload, &sig, "!!!not-b64!!!"),
            Err(VerifyError::InvalidPublicKey)
        ));
        assert!(matches!(
            verify_manifest_bytes(payload, &sig, &STANDARD.encode([0u8; 16])),
            Err(VerifyError::InvalidPublicKey)
        ));
        assert!(sign_manifest_bytes(payload, "bad-key").is_err());
        assert!(sign_manifest_bytes(payload, &STANDARD.encode([0u8; 16])).is_err());
        assert_eq!(EMBEDDED_PUBLIC_KEY_B64, "");
        if option_env!("GATETERM_UPDATER_PUBLIC_KEY").is_none() {
            assert_eq!(compiled_public_key_b64(), "");
        }
    }
}
