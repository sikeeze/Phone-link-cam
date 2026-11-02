//! Sertifikat self-signed (README B1, risiko #1, ADR n001).

use std::net::IpAddr;

use rcgen::{CertificateParams, KeyPair, SanType};
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};

use phonecam_core::error::{Error, Result};

/// Sertifikat + kuncihasil generate.
pub struct SelfSignedCert {
    pub cert_pem: String,
    pub key_pem: String,
}

/// Buat sertifikat self-signed untuk host server (B1).
///
/// SAN mencantumkan setiap IP privat yang di-bind sehingga browser HP
/// could match the cert against the URL `https://<ip>:8443` (risiko #1:
/// iOS Safari mewajibkan HTTPS; prototipe pakai self-signed).
pub fn generate(ips: &[IpAddr]) -> Result<SelfSignedCert> {
    let mut params = CertificateParams::new(vec!["localhost".to_string()])
        .map_err(|e| Error::Tls(format!("params sertifikat tidak valid: {e}")))?;
    params.subject_alt_names = ips.iter().map(|ip| SanType::IpAddress(*ip)).collect();
    // 1 tahun cukup untuk prototipe; bukan CA, tidak ditandatangani.
    params.distinguished_name = rcgen::DistinguishedName::new();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "PhoneCam");

    let key = KeyPair::generate().map_err(|e| Error::Tls(format!("gagal membuat keypair: {e}")))?;
    let cert = params
        .self_signed(&key)
        .map_err(|e| Error::Tls(format!("gagal membuat sertifikat: {e}")))?;

    Ok(SelfSignedCert {
        cert_pem: cert.pem(),
        key_pem: key.serialize_pem(),
    })
}

/// Parse PEM cert menjadi DER (untuk rustls).
pub fn cert_der(pem: &str) -> Result<CertificateDer<'static>> {
    CertificateDer::from_pem_slice(pem.as_bytes())
        .map_err(|e| Error::Tls(format!("gagal parse cert PEM: {e}")))
}

/// Parse PEM key menjadi DER (untuk rustls).
pub fn key_der(pem: &str) -> Result<PrivateKeyDer<'static>> {
    PrivateKeyDer::from_pem_slice(pem.as_bytes())
        .map_err(|e| Error::Tls(format!("gagal parse key PEM: {e}")))
}
