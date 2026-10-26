//! The current user's "Root" store. Its view includes the machine's roots, so a CA trusted for all
//! users counts too; adding to it makes Windows ask the user to confirm.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Security::Cryptography::{
    CertAddEncodedCertificateToStore, CertCloseStore, CertCreateCertificateContext, CertDeleteCertificateFromStore,
    CertFindCertificateInStore, CertFreeCertificateContext, CertOpenStore, CERT_CONTEXT, CERT_FIND_EXISTING,
    CERT_STORE_ADD_REPLACE_EXISTING, CERT_STORE_PROV_SYSTEM_W, CERT_SYSTEM_STORE_CURRENT_USER, HCERTSTORE,
    X509_ASN_ENCODING,
};

use crate::CoreError;

const ERROR_CANCELLED: u32 = 1223;

struct Store(HCERTSTORE);

impl Store {
    fn open() -> Option<Store> {
        let name = widestring::U16CString::from_str_truncate("Root");
        // SAFETY: a system store name as a NUL-terminated wide string; the handle is closed on drop.
        let store = unsafe {
            CertOpenStore(CERT_STORE_PROV_SYSTEM_W, 0, 0, CERT_SYSTEM_STORE_CURRENT_USER, name.as_ptr().cast())
        };
        (!store.is_null()).then_some(Store(store))
    }

    /// The store's copy of `der`, if it holds one (freed by the caller).
    fn find(&self, der: &[u8]) -> Option<*mut CERT_CONTEXT> {
        // SAFETY: `der` outlives both calls; the temporary context is freed before returning.
        unsafe {
            let wanted = CertCreateCertificateContext(X509_ASN_ENCODING, der.as_ptr(), der.len() as u32);
            if wanted.is_null() {
                return None;
            }
            let found =
                CertFindCertificateInStore(self.0, X509_ASN_ENCODING, 0, CERT_FIND_EXISTING, wanted.cast(), null());
            CertFreeCertificateContext(wanted);
            (!found.is_null()).then_some(found)
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // SAFETY: the handle came from CertOpenStore and is closed once.
        unsafe { CertCloseStore(self.0, 0) };
    }
}

pub fn is_trusted(der: &[u8]) -> bool {
    let Some(store) = Store::open() else { return false };
    match store.find(der) {
        Some(found) => {
            // SAFETY: returned by CertFindCertificateInStore.
            unsafe { CertFreeCertificateContext(found) };
            true
        }
        None => false,
    }
}

pub fn trust(der: &[u8]) -> Result<(), CoreError> {
    let store = Store::open().ok_or_else(|| CoreError::Tls("Could not open the certificate store".into()))?;
    // SAFETY: `der` is a valid buffer for the call; no context is requested back.
    let added = unsafe {
        CertAddEncodedCertificateToStore(
            store.0,
            X509_ASN_ENCODING,
            der.as_ptr(),
            der.len() as u32,
            CERT_STORE_ADD_REPLACE_EXISTING,
            null_mut(),
        )
    };
    if added != 0 {
        return Ok(());
    }
    // SAFETY: reads the calling thread's last error.
    let code = unsafe { GetLastError() };
    // "No" in Windows' confirmation dialog.
    if code == ERROR_CANCELLED || code == 0x8007_0000 | ERROR_CANCELLED {
        return Err(CoreError::Tls("Trust was not confirmed".into()));
    }
    Err(CoreError::Tls(format!("Could not trust the Hearth CA (error {code})")))
}

pub fn untrust(der: &[u8]) {
    let Some(store) = Store::open() else { return };
    if let Some(found) = store.find(der) {
        // SAFETY: deleting frees the context returned by the find.
        unsafe { CertDeleteCertificateFromStore(found) };
    }
}
