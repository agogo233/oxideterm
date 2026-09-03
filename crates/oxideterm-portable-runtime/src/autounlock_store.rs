// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! OS storage for the portable auto-unlock key.
//!
//! Windows keeps the token in a DPAPI-protected file next to the vault:
//! DPAPI is bound to the OS user exactly like a Credential Manager entry,
//! but avoids a startup Credential Manager read that behavioral antivirus
//! engines flag as credential theft. Non-Windows platforms keep the native
//! secret store. A legacy Credential Manager entry written by earlier
//! releases is migrated into the file on first read.

use std::path::Path;

use oxideterm_secret_store::NativeSecretStore;
use zeroize::Zeroizing;

#[cfg(target_os = "windows")]
use oxideterm_atomic_file::{durable_remove, durable_write};

use super::keystore::PortableKeystoreError;

const PORTABLE_AUTO_UNLOCK_SERVICE: &str = "com.oxideterm.portable-auto-unlock";

#[cfg(target_os = "windows")]
const AUTO_UNLOCK_TOKEN_FILE_SUFFIX: &str = "autounlock";

/// Fixed application entropy for the DPAPI blob. It is not a secret; it only
/// prevents unrelated DPAPI consumers from decrypting the blob without this
/// exact byte string.
#[cfg(target_os = "windows")]
const AUTO_UNLOCK_DPAPI_ENTROPY: &[u8] = b"OxideTerm.PortableAutoUnlock.v1";

fn legacy_store() -> NativeSecretStore {
    NativeSecretStore::new(PORTABLE_AUTO_UNLOCK_SERVICE)
}

/// Persists the auto-unlock token. On Windows the token is encrypted with
/// DPAPI (user scope + application entropy) and written next to the vault;
/// the legacy keychain entry is left alone because the file write is the only
/// signal that a migration completed.
pub fn store_auto_unlock_token(
    vault_path: &Path,
    account: &str,
    token: &str,
) -> Result<(), PortableKeystoreError> {
    #[cfg(target_os = "windows")]
    {
        // The account name only identifies the legacy keychain entry.
        let _ = account;
        let blob = protect_token(token)?;
        durable_write(&token_file_path(vault_path), blob.as_slice())
            .map_err(PortableKeystoreError::Io)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = vault_path;
        legacy_store()
            .store(account, token)
            .map_err(|_| PortableKeystoreError::CredentialStore)
    }
}

/// Loads the auto-unlock token, returning `None` when automatic unlock is not
/// configured. On Windows a legacy Credential Manager entry is migrated into
/// the DPAPI file once: the file write must succeed before the entry is
/// removed, so a failure leaves the entry as the fallback for the next start.
pub fn take_auto_unlock_token(
    vault_path: &Path,
    account: &str,
) -> Result<Option<Zeroizing<String>>, PortableKeystoreError> {
    #[cfg(target_os = "windows")]
    {
        let path = token_file_path(vault_path);
        if path.exists() {
            return read_protected_token(&path);
        }
        let Some(token) = legacy_store()
            .get(account)
            .map_err(|_| PortableKeystoreError::CredentialStore)?
        else {
            return Ok(None);
        };
        let blob = protect_token(&token)?;
        durable_write(&path, blob.as_slice()).map_err(PortableKeystoreError::Io)?;
        // The file is now authoritative; dropping the legacy entry removes the
        // startup Credential Manager read from all subsequent launches.
        let _ = legacy_store().delete(account);
        Ok(Some(token))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = vault_path;
        legacy_store()
            .get(account)
            .map_err(|_| PortableKeystoreError::CredentialStore)
    }
}

/// Reports whether an auto-unlock token exists without loading it.
pub fn auto_unlock_token_exists(
    vault_path: &Path,
    account: &str,
) -> Result<bool, PortableKeystoreError> {
    #[cfg(target_os = "windows")]
    {
        if token_file_path(vault_path).exists() {
            return Ok(true);
        }
        legacy_store()
            .exists(account)
            .map_err(|_| PortableKeystoreError::CredentialStore)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = vault_path;
        legacy_store()
            .exists(account)
            .map_err(|_| PortableKeystoreError::CredentialStore)
    }
}

/// Removes every stored form of the auto-unlock token. A failure of the
/// primary storage propagates so callers can surface a failed disable, while
/// the legacy keychain cleanup stays best effort because a leftover legacy
/// entry without the file cannot unlock anything by itself.
pub fn delete_auto_unlock_token(
    vault_path: &Path,
    account: &str,
) -> Result<(), PortableKeystoreError> {
    #[cfg(target_os = "windows")]
    durable_remove(&token_file_path(vault_path)).map_err(PortableKeystoreError::Io)?;
    #[cfg(not(target_os = "windows"))]
    let _ = vault_path;
    let _ = legacy_store().delete(account);
    Ok(())
}

#[cfg(target_os = "windows")]
fn token_file_path(vault_path: &Path) -> std::path::PathBuf {
    vault_path.with_extension(AUTO_UNLOCK_TOKEN_FILE_SUFFIX)
}

#[cfg(target_os = "windows")]
fn protect_token(token: &str) -> Result<Zeroizing<Vec<u8>>, PortableKeystoreError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB, CryptProtectData,
    };

    let mut input = Zeroizing::new(token.as_bytes().to_vec());
    let mut entropy = AUTO_UNLOCK_DPAPI_ENTROPY.to_vec();
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_mut_ptr(),
    };
    let entropy_blob = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_mut_ptr(),
    };
    let mut output_blob = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input_blob,
            PCWSTR::null(),
            Some(&entropy_blob),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output_blob,
        )
        .map_err(|_| PortableKeystoreError::CredentialStore)?;
    }
    // The blob is ciphertext, but copying into a wiping container keeps every
    // token representation owned by zeroizing types.
    let blob = Zeroizing::new(unsafe {
        std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec()
    });
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output_blob.pbData.cast())));
    }
    Ok(blob)
}

#[cfg(target_os = "windows")]
fn read_protected_token(path: &Path) -> Result<Option<Zeroizing<String>>, PortableKeystoreError> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB, CryptUnprotectData,
    };

    let mut blob = match std::fs::read(path) {
        Ok(blob) => blob,
        // A missing or unreadable file is equivalent to "not configured": the
        // password unlock stays available.
        Err(_) => return Ok(None),
    };
    let mut entropy = AUTO_UNLOCK_DPAPI_ENTROPY.to_vec();
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_mut_ptr(),
    };
    let entropy_blob = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_mut_ptr(),
    };
    let mut output_blob = CRYPT_INTEGER_BLOB::default();
    let unprotected = unsafe {
        CryptUnprotectData(
            &input_blob,
            None,
            Some(&entropy_blob),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output_blob,
        )
    };
    if unprotected.is_err() {
        // The blob does not decrypt on this machine or user (for example the
        // portable directory was moved). Delete it so later launches do not
        // repeat the failure, and fall back to the password prompt.
        let _ = durable_remove(path);
        return Ok(None);
    }
    let token = Zeroizing::new(unsafe {
        String::from_utf8_lossy(std::slice::from_raw_parts(
            output_blob.pbData,
            output_blob.cbData as usize,
        ))
        .into_owned()
    });
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output_blob.pbData.cast())));
    }
    Ok(Some(token))
}
