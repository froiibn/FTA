//! Pinned certificates still verify TLS handshake signatures; pairing alone may accept a new pin.
use crate::{
    Error, Result,
    config::{AppPaths, atomic_write},
};
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, sync::Arc};

/// The private key is never exposed by API responses, Debug output, or serialized models.
#[derive(Clone)]
pub struct Identity {
    pub cert: Vec<u8>,
    key: Vec<u8>,
}
#[derive(Serialize, Deserialize)]
struct DiskIdentity {
    cert: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keychain_account: Option<String>,
}
impl Drop for Identity {
    fn drop(&mut self) {
        // The native TLS configuration owns its copies; erase this engine-owned key copy too.
        use zeroize::Zeroize;
        self.key.zeroize();
    }
}
impl Identity {
    /// Tests inject an in-memory identity instead of creating hundreds of native Keychain entries.
    pub fn generate_ephemeral() -> Result<Self> {
        let generated = rcgen::generate_simple_self_signed(vec!["fta.local".into()])
            .map_err(|e| Error::new("IDENTITY_ERROR", e.to_string()))?;
        Ok(Self {
            cert: generated.cert.der().to_vec(),
            key: generated.signing_key.serialize_der(),
        })
    }
    /// Renew certificate metadata without changing the device's authenticated public key.
    /// Persisting the result is a separate explicit action; this never rotates or trusts a new key.
    pub fn renew_certificate(&self) -> Result<Self> {
        let key_pair = rcgen::KeyPair::try_from(self.key.as_slice())
            .map_err(|error| Error::new("IDENTITY_ERROR", error.to_string()))?;
        let mut params = rcgen::CertificateParams::new(vec!["fta.local".into()])
            .map_err(|error| Error::new("IDENTITY_ERROR", error.to_string()))?;
        params.serial_number = Some(rcgen::SerialNumber::from_slice(
            uuid::Uuid::new_v4().as_bytes(),
        ));
        let certificate = params
            .self_signed(&key_pair)
            .map_err(|error| Error::new("IDENTITY_ERROR", error.to_string()))?;
        Ok(Self {
            cert: certificate.der().to_vec(),
            key: self.key.clone(),
        })
    }
    /// Native key-protection failures are surfaced, never replaced with a fresh trusted identity.
    pub fn load(paths: &AppPaths) -> Result<Self> {
        let file = paths.state_dir.join("identity.json");
        if file.exists() {
            let disk: DiskIdentity = serde_json::from_slice(&fs::read(&file)?)?;
            let cert =
                hex::decode(&disk.cert).map_err(|e| Error::new("IDENTITY_ERROR", e.to_string()))?;
            #[cfg(target_os = "macos")]
            let key = if let Some(account) = &disk.keychain_account {
                fta_platform::macos::load_identity_key(account)?
            } else {
                // Migrate early-development files once; later loads must never fall back to disk.
                let key = decode_disk_key(&disk)?;
                let account = fingerprint(&cert);
                fta_platform::macos::store_identity_key(&account, &key)?;
                atomic_write(
                    &file,
                    &serde_json::to_vec(&DiskIdentity {
                        cert: disk.cert,
                        key: None,
                        keychain_account: Some(account),
                    })?,
                )?;
                key
            };
            #[cfg(windows)]
            let key = fta_platform::windows::unprotect_secret(&decode_disk_key(&disk)?)?;
            #[cfg(not(any(windows, target_os = "macos")))]
            let key = decode_disk_key(&disk)?;
            let identity = Self { cert, key };
            // Parsing and matching cert/key now prevents a broken identity from looking online.
            identity.server(Vec::new(), true)?;
            return Ok(identity);
        }
        let identity = Self::generate_ephemeral()?;
        #[cfg(target_os = "macos")]
        let disk = {
            let account = fingerprint(&identity.cert);
            fta_platform::macos::store_identity_key(&account, &identity.key)?;
            DiskIdentity {
                cert: hex::encode(&identity.cert),
                key: None,
                keychain_account: Some(account),
            }
        };
        #[cfg(windows)]
        let disk = DiskIdentity {
            cert: hex::encode(&identity.cert),
            key: Some(hex::encode(fta_platform::windows::protect_secret(
                &identity.key,
            )?)),
            keychain_account: None,
        };
        #[cfg(not(any(windows, target_os = "macos")))]
        let disk = DiskIdentity {
            cert: hex::encode(&identity.cert),
            key: Some(hex::encode(&identity.key)),
            keychain_account: None,
        };
        atomic_write(&file, &serde_json::to_vec(&disk)?)?;
        Ok(identity)
    }
    fn key(&self) -> PrivateKeyDer<'static> {
        PrivatePkcs8KeyDer::from(self.key.clone()).into()
    }
    pub fn server(&self, pins: Vec<Vec<u8>>, pairing: bool) -> Result<rustls::ServerConfig> {
        let builder = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(tls_error)?;
        // Accept a new pin during pairing, but still demand proof of its private key.
        let builder = builder.with_client_cert_verifier(Arc::new(PinVerifier {
            pins,
            allow_new: pairing,
        }));
        builder
            .with_single_cert(vec![CertificateDer::from(self.cert.clone())], self.key())
            .map_err(tls_error)
    }
    pub fn client(&self, pins: Vec<Vec<u8>>, pairing: bool) -> Result<rustls::ClientConfig> {
        rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(tls_error)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinVerifier {
            pins,
            allow_new: pairing,
        }))
        .with_client_auth_cert(vec![CertificateDer::from(self.cert.clone())], self.key())
        .map_err(tls_error)
    }
}
// Linux stores this field only in a mode-0600 file; Windows stores a DPAPI ciphertext.
fn decode_disk_key(disk: &DiskIdentity) -> Result<Vec<u8>> {
    let key = disk
        .key
        .as_deref()
        .ok_or_else(|| Error::new("IDENTITY_ERROR", "identity key is missing"))?;
    hex::decode(key).map_err(|e| Error::new("IDENTITY_ERROR", e.to_string()))
}
pub fn fingerprint(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
/// Pin the complete DER SubjectPublicKeyInfo, including its algorithm identifier.
/// The validated parser avoids extracting ASN.1 fields with ad-hoc byte offsets.
pub fn public_key_fingerprint(certificate: &[u8]) -> Result<String> {
    let (remainder, certificate) = x509_parser::parse_x509_certificate(certificate)
        .map_err(|_| Error::new("IDENTITY_ERROR", "invalid X.509 certificate"))?;
    if !remainder.is_empty() {
        return Err(Error::new(
            "IDENTITY_ERROR",
            "trailing data after certificate",
        ));
    }
    Ok(fingerprint(certificate.public_key().raw))
}
/// Certificate renewal is accepted only when its SPKI is identical to a registered key.
pub fn same_public_key(first: &[u8], second: &[u8]) -> bool {
    match (
        public_key_fingerprint(first),
        public_key_fingerprint(second),
    ) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}
fn tls_error(e: rustls::Error) -> Error {
    Error::new("TLS_ERROR", e.to_string())
}

#[derive(Debug)]
struct PinVerifier {
    pins: Vec<Vec<u8>>,
    allow_new: bool,
}
impl PinVerifier {
    fn check(&self, cert: &CertificateDer<'_>) -> std::result::Result<(), rustls::Error> {
        if self.allow_new
            || self
                .pins
                .iter()
                .any(|pin| same_public_key(pin, cert.as_ref()))
        {
            Ok(())
        } else {
            Err(rustls::Error::General(
                "certificate is not registered".into(),
            ))
        }
    }
    fn schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
    fn signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            signature,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
}
impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        self.check(end)?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        s: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.signature(m, c, s)
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        s: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.signature(m, c, s)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes()
    }
}
impl ClientCertVerifier for PinVerifier {
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        end: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        self.check(end)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        s: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.signature(m, c, s)
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        s: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.signature(m, c, s)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A peer must prove possession of its key even before its certificate is registered.
    #[tokio::test]
    async fn pairing_tls_rejects_clients_without_certificates() {
        let server_identity = Identity::generate_ephemeral().unwrap();
        let server_config = server_identity.server(Vec::new(), true).unwrap();
        let client_config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinVerifier {
            pins: vec![server_identity.cert.clone()],
            allow_new: false,
        }))
        .with_no_client_auth();
        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));
        let connector = tokio_rustls::TlsConnector::from(Arc::new(client_config));
        let (server, _) = tokio::join!(
            acceptor.accept(server_io),
            connector.connect(
                ServerName::try_from("fta.local").unwrap().to_owned(),
                client_io
            )
        );
        assert!(server.is_err());
    }

    // A matching public certificate cannot authorize use of a different private key.
    #[test]
    fn mismatched_private_key_is_rejected_before_listening() {
        let first = Identity::generate_ephemeral().unwrap();
        let second = Identity::generate_ephemeral().unwrap();
        let mismatched = Identity {
            cert: first.cert.clone(),
            key: second.key.clone(),
        };
        assert!(mismatched.server(Vec::new(), true).is_err());
    }
    // Renewed certificate metadata must not break either side of a pinned mutual TLS connection.
    #[tokio::test]
    async fn same_key_certificate_renewal_keeps_mutual_tls_trust() {
        let server = Identity::generate_ephemeral().unwrap();
        let client = Identity::generate_ephemeral().unwrap();
        let renewed_server = server.renew_certificate().unwrap();
        let renewed_client = client.renew_certificate().unwrap();
        assert_ne!(server.cert, renewed_server.cert);
        assert!(same_public_key(&server.cert, &renewed_server.cert));
        assert_eq!(
            public_key_fingerprint(&client.cert).unwrap(),
            public_key_fingerprint(&renewed_client.cert).unwrap()
        );
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(
            renewed_server
                .server(vec![client.cert.clone()], false)
                .unwrap(),
        ));
        let connector = tokio_rustls::TlsConnector::from(Arc::new(
            renewed_client
                .client(vec![server.cert.clone()], false)
                .unwrap(),
        ));
        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        let (server_result, client_result) = tokio::join!(
            acceptor.accept(server_io),
            connector.connect(
                ServerName::try_from("fta.local").unwrap().to_owned(),
                client_io
            )
        );
        assert!(server_result.is_ok());
        assert!(client_result.is_ok());
        let attacker = Identity::generate_ephemeral().unwrap();
        assert!(!same_public_key(&server.cert, &attacker.cert));
        let mut trailing = server.cert.clone();
        trailing.push(0);
        assert!(public_key_fingerprint(&trailing).is_err());
    }
    // A valid certificate with a fresh key must still fail actual TLS authentication.
    #[tokio::test]
    async fn new_key_cannot_reuse_existing_registration() {
        let server = Identity::generate_ephemeral().unwrap();
        let registered = Identity::generate_ephemeral().unwrap();
        let attacker = Identity::generate_ephemeral().unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(
            server.server(vec![registered.cert.clone()], false).unwrap(),
        ));
        let connector = tokio_rustls::TlsConnector::from(Arc::new(
            attacker.client(vec![server.cert.clone()], false).unwrap(),
        ));
        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        let (server_result, _) = tokio::join!(
            acceptor.accept(server_io),
            connector.connect(
                ServerName::try_from("fta.local").unwrap().to_owned(),
                client_io
            )
        );
        assert!(server_result.is_err());
    }
}
