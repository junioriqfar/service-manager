use std::ffi::c_void;

use thiserror::Error;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

#[derive(Debug, Error)]
pub enum DpapiError {
    #[error("Operasi DPAPI gagal: {0}")]
    Windows(String),
}

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: if data.is_empty() {
            std::ptr::null_mut()
        } else {
            data.as_ptr() as *mut u8
        },
    }
}

fn entropy_blob(entropy: Option<&[u8]>) -> Option<CRYPT_INTEGER_BLOB> {
    entropy.map(blob)
}

fn copy_and_free(output: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if output.pbData.is_null() || output.cbData == 0 {
        return Vec::new();
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast::<c_void>())));
    }
    bytes
}

/// Enkripsi data dengan Windows DPAPI (scope user + machine saat ini).
pub fn protect(data: &[u8], entropy: Option<&[u8]>) -> Result<Vec<u8>, DpapiError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let input = blob(data);
    let entropy = entropy_blob(entropy);
    let entropy_ptr = entropy
        .as_ref()
        .map(|value| value as *const CRYPT_INTEGER_BLOB);
    let mut output = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            entropy_ptr,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|error| DpapiError::Windows(error.to_string()))?;

    Ok(copy_and_free(output))
}

/// Dekripsi data DPAPI yang dibuat oleh user/machine yang sama.
pub fn unprotect(data: &[u8], entropy: Option<&[u8]>) -> Result<Vec<u8>, DpapiError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let input = blob(data);
    let entropy = entropy_blob(entropy);
    let entropy_ptr = entropy
        .as_ref()
        .map(|value| value as *const CRYPT_INTEGER_BLOB);
    let mut output = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptUnprotectData(
            &input,
            None,
            entropy_ptr,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|error| DpapiError::Windows(error.to_string()))?;

    Ok(copy_and_free(output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_without_entropy() {
        let secret = b"super-secret-token";
        let encrypted = protect(secret, None).unwrap();
        assert_ne!(encrypted, secret);
        assert_eq!(unprotect(&encrypted, None).unwrap(), secret);
    }

    #[test]
    fn roundtrip_with_entropy() {
        let secret = b"another-secret";
        let entropy = b"service-manager";
        let encrypted = protect(secret, Some(entropy)).unwrap();
        assert_eq!(unprotect(&encrypted, Some(entropy)).unwrap(), secret);
        assert!(unprotect(&encrypted, Some(b"wrong-entropy")).is_err());
    }

    #[test]
    fn roundtrip_empty_payload() {
        let encrypted = protect(b"", None).unwrap();
        assert!(unprotect(&encrypted, None).unwrap().is_empty());
    }
}
