//! The local CA (created once, valid 10 years) and one leaf certificate per secured site (valid
//! 397 days, renewed when fewer than 30 days remain).

use std::path::{Path, PathBuf};

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    PublicKeyData,
};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use x509_parser::prelude::{parse_x509_pem, FromDer, X509Certificate};

use crate::{atomic, CoreError, HearthPaths};

const CA_DAYS: i64 = 3650;
/// The most browsers accept for a server certificate.
const LEAF_DAYS: i64 = 397;
const RENEW_BEFORE_DAYS: i64 = 30;

pub struct Ca {
    cert_pem: String,
    key: KeyPair,
    der: Vec<u8>,
}

impl Ca {
    pub fn der(&self) -> &[u8] {
        &self.der
    }

    /// SHA-256 of the certificate, hex, as shown by browsers.
    pub fn fingerprint(&self) -> String {
        Sha256::digest(&self.der).iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeafState {
    Unchanged,
    Issued,
}

fn dir(paths: &HearthPaths) -> PathBuf {
    paths.data.join("tls")
}

/// The certificate and key files of a site (`name` without `.test`).
pub fn leaf_paths(paths: &HearthPaths, name: &str) -> (PathBuf, PathBuf) {
    let sites = dir(paths).join("sites");
    (sites.join(format!("{name}.crt")), sites.join(format!("{name}.key")))
}

/// Loads the CA, or creates it on first use.
pub fn ensure_ca(paths: &HearthPaths) -> Result<Ca, CoreError> {
    let crt = dir(paths).join("ca.crt");
    let key = dir(paths).join("ca.key");
    if let Some(ca) = load_ca(&crt, &key) {
        return Ok(ca);
    }
    let key_pair = KeyPair::generate().map_err(failed)?;
    let mut params = CertificateParams::new(Vec::<String>::new()).map_err(failed)?;
    let host = whoami::fallible::hostname().unwrap_or_else(|_| "this computer".into());
    params.distinguished_name.push(DnType::OrganizationName, "Hearth");
    params.distinguished_name.push(DnType::CommonName, format!("Hearth Local CA ({}@{host})", whoami::username()));
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(CA_DAYS);
    let cert = params.self_signed(&key_pair).map_err(failed)?;
    create_dir(&dir(paths))?;
    atomic::write_private(&key, key_pair.serialize_pem().as_bytes())?;
    atomic::write(&crt, cert.pem().as_bytes())?;
    Ok(Ca { cert_pem: cert.pem(), key: key_pair, der: cert.der().to_vec() })
}

fn create_dir(dir: &Path) -> Result<(), CoreError> {
    std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))
}

fn failed(e: rcgen::Error) -> CoreError {
    CoreError::Tls(format!("Could not create a certificate: {e}"))
}

fn load_ca(crt: &Path, key: &Path) -> Option<Ca> {
    let cert_pem = std::fs::read_to_string(crt).ok()?;
    let key = KeyPair::from_pem(&std::fs::read_to_string(key).ok()?).ok()?;
    let der = parse_x509_pem(cert_pem.as_bytes()).ok()?.1.contents;
    // A certificate that does not belong to the key is no CA we can sign with.
    let (_, cert) = X509Certificate::from_der(&der).ok()?;
    (cert.public_key().raw == key.subject_public_key_info().as_slice()).then_some(Ca { cert_pem, key, der })
}

/// Whether the leaf at `crt` is signed by `ca` and still valid for more than the renewal window.
fn leaf_is_current(crt: &Path, key: &Path, ca: &Ca, now: OffsetDateTime) -> bool {
    let (Ok(pem), true) = (std::fs::read(crt), key.is_file()) else { return false };
    let Ok((_, pem)) = parse_x509_pem(&pem) else { return false };
    let Ok(leaf) = pem.parse_x509() else { return false };
    let Ok((_, ca_cert)) = X509Certificate::from_der(&ca.der) else { return false };
    leaf.verify_signature(Some(ca_cert.public_key())).is_ok()
        && leaf.validity().not_after.to_datetime() - now > Duration::days(RENEW_BEFORE_DAYS)
}

/// Issues the site's certificate when it is missing, unreadable, signed by another CA, or expires
/// within 30 days of `now`.
pub fn ensure_leaf(paths: &HearthPaths, ca: &Ca, name: &str, now: OffsetDateTime) -> Result<LeafState, CoreError> {
    let (crt, key) = leaf_paths(paths, name);
    if leaf_is_current(&crt, &key, ca, now) {
        return Ok(LeafState::Unchanged);
    }
    let host = format!("{name}.test");
    let mut params = CertificateParams::new(vec![host.clone(), format!("*.{host}")]).map_err(failed)?;
    params.distinguished_name.push(DnType::CommonName, host);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature, KeyUsagePurpose::KeyEncipherment];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.use_authority_key_identifier_extension = true;
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(LEAF_DAYS);
    let issuer = Issuer::from_ca_cert_pem(&ca.cert_pem, &ca.key).map_err(failed)?;
    let key_pair = KeyPair::generate().map_err(failed)?;
    let cert = params.signed_by(&key_pair, &issuer).map_err(failed)?;
    create_dir(crt.parent().unwrap_or(Path::new(".")))?;
    atomic::write_private(&key, key_pair.serialize_pem().as_bytes())?;
    atomic::write(&crt, cert.pem().as_bytes())?;
    Ok(LeafState::Issued)
}

pub fn remove_leaf(paths: &HearthPaths, name: &str) {
    let (crt, key) = leaf_paths(paths, name);
    let _ = std::fs::remove_file(crt);
    let _ = std::fs::remove_file(key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::time::Duration;
    use x509_parser::prelude::{parse_x509_pem, FromDer, X509Certificate};

    fn home() -> (tempfile::TempDir, HearthPaths) {
        let d = tempfile::tempdir().unwrap();
        let p = HearthPaths::at(d.path().join("hearth"));
        p.ensure().unwrap();
        (d, p)
    }

    fn not_after(paths: &HearthPaths, name: &str) -> OffsetDateTime {
        let pem = std::fs::read(leaf_paths(paths, name).0).unwrap();
        let (_, pem) = parse_x509_pem(&pem).unwrap();
        pem.parse_x509().unwrap().validity().not_after.to_datetime()
    }

    #[test]
    fn the_ca_is_created_once() {
        let (_d, p) = home();
        let first = ensure_ca(&p).unwrap().fingerprint();
        assert_eq!(first.len(), 64);
        assert_eq!(ensure_ca(&p).unwrap().fingerprint(), first);
    }

    #[test]
    fn a_leaf_names_the_site_and_its_subdomains_and_is_signed_by_the_ca() {
        let (_d, p) = home();
        let ca = ensure_ca(&p).unwrap();
        assert_eq!(ensure_leaf(&p, &ca, "shop", OffsetDateTime::now_utc()).unwrap(), LeafState::Issued);
        let pem = std::fs::read(leaf_paths(&p, "shop").0).unwrap();
        let (_, pem) = parse_x509_pem(&pem).unwrap();
        let leaf = pem.parse_x509().unwrap();
        let san = leaf.subject_alternative_name().unwrap().unwrap().value;
        let names: Vec<String> = san.general_names.iter().map(|n| n.to_string()).collect();
        assert_eq!(names, ["DNSName(shop.test)", "DNSName(*.shop.test)"]);
        let (_, ca_cert) = X509Certificate::from_der(ca.der()).unwrap();
        leaf.verify_signature(Some(ca_cert.public_key())).unwrap();
        assert!(leaf_paths(&p, "shop").1.is_file());
    }

    #[test]
    fn a_leaf_is_renewed_when_fewer_than_30_days_remain() {
        let (_d, p) = home();
        let ca = ensure_ca(&p).unwrap();
        let now = OffsetDateTime::now_utc();
        ensure_leaf(&p, &ca, "shop", now).unwrap();
        assert_eq!(ensure_leaf(&p, &ca, "shop", now).unwrap(), LeafState::Unchanged);
        let end = not_after(&p, "shop");
        assert_eq!(ensure_leaf(&p, &ca, "shop", end - Duration::days(31)).unwrap(), LeafState::Unchanged);
        assert_eq!(ensure_leaf(&p, &ca, "shop", end - Duration::days(29)).unwrap(), LeafState::Issued);
    }

    #[test]
    fn a_deleted_leaf_is_issued_again() {
        let (_d, p) = home();
        let ca = ensure_ca(&p).unwrap();
        ensure_leaf(&p, &ca, "shop", OffsetDateTime::now_utc()).unwrap();
        std::fs::remove_file(leaf_paths(&p, "shop").0).unwrap();
        assert_eq!(ensure_leaf(&p, &ca, "shop", OffsetDateTime::now_utc()).unwrap(), LeafState::Issued);
    }

    #[test]
    fn a_leaf_from_a_replaced_ca_is_issued_again() {
        let (_d, p) = home();
        let ca = ensure_ca(&p).unwrap();
        ensure_leaf(&p, &ca, "shop", OffsetDateTime::now_utc()).unwrap();
        std::fs::remove_file(p.data.join("tls").join("ca.crt")).ok();
        let other = ensure_ca(&p).unwrap();
        assert_ne!(other.fingerprint(), ca.fingerprint());
        assert_eq!(ensure_leaf(&p, &other, "shop", OffsetDateTime::now_utc()).unwrap(), LeafState::Issued);
    }

    #[test]
    fn remove_leaf_deletes_both_files() {
        let (_d, p) = home();
        let ca = ensure_ca(&p).unwrap();
        ensure_leaf(&p, &ca, "shop", OffsetDateTime::now_utc()).unwrap();
        remove_leaf(&p, "shop");
        assert!(!leaf_paths(&p, "shop").0.exists() && !leaf_paths(&p, "shop").1.exists());
    }

    #[cfg(unix)]
    #[test]
    fn keys_are_readable_by_the_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, p) = home();
        let ca = ensure_ca(&p).unwrap();
        ensure_leaf(&p, &ca, "shop", OffsetDateTime::now_utc()).unwrap();
        for key in [p.data.join("tls").join("ca.key"), leaf_paths(&p, "shop").1] {
            assert_eq!(std::fs::metadata(&key).unwrap().permissions().mode() & 0o777, 0o600, "{}", key.display());
        }
    }
}
