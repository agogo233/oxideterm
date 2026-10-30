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
use zeroize::Zeroize;

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
/// the legacy keychain entry is removed afterwards so later launches never
/// read the credential manager.
pub fn store_auto_unlock_token(
    vault_path: &Path,
    account: &str,
    token: &str,
) -> Result<(), PortableKeystoreError> {
    #[cfg(target_os = "windows")]
    {
        let blob = protect_token(token)?;
        durable_write(&token_file_path(vault_path), blob.as_slice())
            .map_err(PortableKeystoreError::Io)?;
        remove_legacy_entry(account);
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = vault_path;
        legacy_store()
            .store(account, token)
            .map_err(|_| PortableKeystoreError::CredentialStore)
    }
}

/// Removes the legacy credential manager entry. The deletion is best effort:
/// a leftover entry cannot unlock the vault on its own, and `delete_auto_unlock_token`
/// retries the cleanup when the portable keystore is removed.
#[cfg(target_os = "windows")]
fn remove_legacy_entry(account: &str) {
    let _ = legacy_store().delete(account);
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
        match read_protected_token(&path) {
            // A readable file is the single source of truth; it is also the
            // only case that returns without touching the credential manager.
            Ok(Some(token)) => return Ok(Some(token)),
            // An absent file is the only case that still consults the legacy
            // entry; a read or decryption failure is a real error.
            Ok(None) => {}
            Err(error) => return Err(error),
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
        remove_legacy_entry(account);
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
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };
    use windows::core::PCWSTR;

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

    let mut blob = match std::fs::read(path) {
        Ok(blob) => blob,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        // An unreadable file is an I/O problem rather than a missing
        // credential, so the stored blob stays untouched for a later retry.
        Err(error) => return Err(PortableKeystoreError::Io(error)),
    };
    let mut entropy = AUTO_UNLOCK_DPAPI_ENTROPY.to_vec();
    let output_blob = match unprotect_blob(&mut blob, &mut entropy) {
        Some(output_blob) => output_blob,
        // A single failure can come from a temporarily unavailable user
        // profile, so the decryption is retried before the blob is dropped.
        None => match unprotect_blob(&mut blob, &mut entropy) {
            Some(output_blob) => output_blob,
            None => {
                // The blob does not decrypt on this machine or user (for
                // example the portable directory was moved). Delete it so later
                // launches do not repeat the failure, and report the removal.
                match durable_remove(path) {
                    Ok(()) => {
                        return Err(PortableKeystoreError::InvalidAutoUnlockCredential);
                    }
                    // If the cleanup fails, surface the I/O error instead of
                    // claiming a stale blob that still exists was removed.
                    Err(error) => return Err(PortableKeystoreError::Io(error)),
                }
            }
        },
    };
    let plaintext =
        unsafe { std::slice::from_raw_parts_mut(output_blob.pbData, output_blob.cbData as usize) };
    let token = match std::str::from_utf8(plaintext) {
        Ok(text) => Zeroizing::new(text.to_owned()),
        Err(_) => {
            plaintext.zeroize();
            unsafe {
                let _ = LocalFree(Some(HLOCAL(output_blob.pbData.cast())));
            }
            return Err(PortableKeystoreError::InvalidAutoUnlockCredential);
        }
    };
    // The plaintext buffer belongs to DPAPI, so it is wiped before the handle
    // is released back to the allocator.
    plaintext.zeroize();
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output_blob.pbData.cast())));
    }
    Ok(Some(token))
}

#[cfg(target_os = "windows")]
fn unprotect_blob(
    blob: &mut [u8],
    entropy: &mut [u8],
) -> Option<windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB> {
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

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
    unprotected.is_ok().then_some(output_blob)
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use crate::PORTABLE_KEYSTORE_FILENAME;

    #[test]
    fn auto_unlock_entropy_is_versioned_and_stable() {
        // The entropy is part of the stored blob format, so changing it would
        // invalidate every already-migrated portable vault.
        assert_eq!(
            AUTO_UNLOCK_DPAPI_ENTROPY,
            b"OxideTerm.PortableAutoUnlock.v1"
        );
    }

    #[test]
    fn token_file_replaces_the_vault_extension() {
        let vault = std::path::Path::new("C:\\OxideTerm").join(PORTABLE_KEYSTORE_FILENAME);
        let path = token_file_path(&vault);
        // The directory is preserved and only the extension is swapped.
        assert_eq!(
            path,
            std::path::Path::new("C:\\OxideTerm\\keystore.autounlock")
        );
    }
}
