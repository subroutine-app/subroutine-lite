use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;
use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
        Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    },
    core::PCWSTR,
};
use zeroize::Zeroize;

use super::{Credentials, KeyringCredentials};

const HEADER: &[u8] = b"SubroutineLiteSession\x01";
const MAX_SESSION_BYTES: usize = 1024 * 1024;
const MAX_FILE_BYTES: usize = MAX_SESSION_BYTES + 64 * 1024;

pub(super) struct OsCredentials(pub(super) String);

impl OsCredentials {
    fn file(&self) -> Result<ProtectedSession, String> {
        // Credentials remain tied to this Windows user, not to a relocatable/shared workspace.
        let root = dirs::data_local_dir()
            .ok_or("Windows did not provide a local application data directory.")?
            .join("Subroutine Lite")
            .join("auth");
        Ok(ProtectedSession::new(&root, &self.0))
    }
}

impl Credentials for OsCredentials {
    fn load(&self) -> Result<Option<Vec<u8>>, String> {
        self.file()?.load(&KeyringCredentials(self.0.clone()))
    }

    fn save(&self, bytes: &[u8]) -> Result<(), String> {
        self.file()?
            .save(bytes, &KeyringCredentials(self.0.clone()))
    }

    fn delete(&self) -> Result<(), String> {
        self.file()?.delete(&KeyringCredentials(self.0.clone()))
    }
}

struct ProtectedSession {
    path: PathBuf,
    entropy: Vec<u8>,
}

impl ProtectedSession {
    fn new(root: &Path, identity: &str) -> Self {
        Self {
            path: root.join(format!("{:x}.dpapi", Sha256::digest(identity.as_bytes()))),
            entropy: format!("com.subroutine.SubroutineLite/session/v1/{identity}").into_bytes(),
        }
    }

    fn load(&self, legacy: &dyn Credentials) -> Result<Option<Vec<u8>>, String> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let Some(bytes) = legacy.load()? else {
                    return Ok(None);
                };
                self.save(&bytes, legacy)?;
                return Ok(Some(bytes));
            }
            Err(error) => return Err(format!("Cannot read the encrypted session: {error}")),
        };
        let mut encoded = Vec::new();
        file.take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut encoded)
            .map_err(|error| format!("Cannot read the encrypted session: {error}"))?;
        if encoded.len() > MAX_FILE_BYTES || !encoded.starts_with(HEADER) {
            return Err(
                "The encrypted session has an invalid format. Sign out, then sign in again.".into(),
            );
        }
        // An existing file is authoritative, even if corrupt or a token-free refresh checkpoint.
        // Never resurrect a legacy refresh token when reading it fails.
        let bytes = self.crypt(&encoded[HEADER.len()..], false)?;
        Self::retire_legacy(legacy)?;
        Ok(Some(bytes))
    }

    fn save(&self, bytes: &[u8], legacy: &dyn Credentials) -> Result<(), String> {
        if bytes.len() > MAX_SESSION_BYTES {
            return Err("The session is too large to save securely (maximum 1 MiB).".into());
        }
        let encrypted = self.crypt(bytes, true)?;
        let parent = self.path.parent().expect("session path has a parent");
        fs::create_dir_all(parent)
            .map_err(|error| format!("Cannot create the encrypted session directory: {error}"))?;
        let temporary = self.path.with_extension(format!("{}.tmp", Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(HEADER)?;
            file.write_all(&encrypted)?;
            file.sync_all()?;
            drop(file);
            replace(&temporary, &self.path)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(format!(
                "Cannot save the encrypted session: {error}. Check folder permissions and free disk space; sync is paused."
            ));
        }
        // Do not allow refresh until the old representation has been retired. If cleanup fails,
        // the new file stays authoritative and load/save will retry cleanup before using tokens.
        Self::retire_legacy(legacy)
    }

    fn delete(&self, legacy: &dyn Credentials) -> Result<(), String> {
        // Delete legacy first: otherwise an interrupted sign-out could import it on next launch.
        Self::retire_legacy(legacy)?;
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "Cannot remove the encrypted session: {error}. Retry Sign Out before quitting."
            )),
        }
    }

    fn retire_legacy(legacy: &dyn Credentials) -> Result<(), String> {
        legacy.delete().map_err(|error| {
            format!("Cannot retire the legacy saved session; sync is paused. {error}")
        })
    }

    fn crypt(&self, bytes: &[u8], protect: bool) -> Result<Vec<u8>, String> {
        let input = blob(bytes)?;
        let entropy = blob(&self.entropy)?;
        let mut output = DpapiOutput(CRYPT_INTEGER_BLOB::default());
        // DPAPI does not mutate input buffers. Omitting LOCAL_MACHINE binds protection to the
        // current Windows user; the entropy also binds ciphertext to this app/configuration.
        let result = unsafe {
            if protect {
                CryptProtectData(
                    &input,
                    PCWSTR::null(),
                    Some(&entropy),
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output.0,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    None,
                    Some(&entropy),
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output.0,
                )
            }
        };
        result.map_err(|error| {
            let operation = if protect { "encrypt" } else { "decrypt" };
            format!(
                "Windows could not {operation} the saved session ({}). Sign out and sign in again using this Windows profile.",
                error.code()
            )
        })?;
        if output.0.cbData == 0 {
            return Ok(Vec::new());
        }
        Ok(
            unsafe { std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize) }
                .to_vec(),
        )
    }
}

fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, String> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: bytes
            .len()
            .try_into()
            .map_err(|_| "Session exceeds the Windows encryption limit.")?,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

struct DpapiOutput(CRYPT_INTEGER_BLOB);

impl Drop for DpapiOutput {
    fn drop(&mut self) {
        if !self.0.pbData.is_null() {
            unsafe {
                std::slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize).zeroize();
                let _ = LocalFree(Some(HLOCAL(self.0.pbData.cast())));
            }
        }
    }
}

fn replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Same-directory replacement keeps complete snapshots atomic, including refresh checkpoints.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xffff))
}

#[cfg(test)]
#[path = "windows_credentials_tests.rs"]
mod tests;
