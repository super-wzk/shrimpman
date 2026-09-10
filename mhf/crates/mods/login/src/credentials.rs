use crate::model::PasswordCredentials;
use std::{ffi::CStr, ptr};
use windows::{
    Win32::{
        Foundation::ERROR_NOT_FOUND,
        Security::Credentials::{
            CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE, CRED_TYPE_DOMAIN_PASSWORD, CRED_TYPE_GENERIC,
            CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
        },
        System::LibraryLoader::{GetModuleHandleA, GetProcAddress},
    },
    core::{HRESULT, PCSTR, PCWSTR, PWSTR},
};

const NTDLL: &CStr = c"ntdll.dll";
const WINE_GET_VERSION: &CStr = c"wine_get_version";
const TARGET_PREFIX: &str = "Shrimpman MHF — ";

#[derive(Clone)]
pub(crate) struct CredentialStore {
    target: String,
    credential_type: CRED_TYPE,
}

impl CredentialStore {
    pub(crate) fn new(sign_endpoint: &str) -> Self {
        let endpoint = sign_endpoint.trim().trim_end_matches('/');
        // Wine bridges domain-password credentials to the host keychain. On native Windows this
        // is an application credential, so the generic type matches its semantics.
        let credential_type = if is_wine() {
            CRED_TYPE_DOMAIN_PASSWORD
        } else {
            CRED_TYPE_GENERIC
        };
        Self {
            target: format!("{TARGET_PREFIX}{endpoint}"),
            credential_type,
        }
    }

    pub(crate) fn read(&self) -> Result<Option<PasswordCredentials>, String> {
        let target = wide_string(&self.target)?;
        let mut raw = ptr::null_mut();
        if let Err(error) = unsafe {
            CredReadW(
                PCWSTR(target.as_ptr()),
                self.credential_type,
                None,
                &mut raw,
            )
        } {
            return if is_not_found(&error) {
                Ok(None)
            } else {
                Err(format!(
                    "Could not load the saved password from the system credential store: {error}"
                ))
            };
        }

        let credential = CredentialBuffer(raw);
        let credential = credential
            .get()
            .ok_or_else(|| "The system credential store returned an empty credential".to_owned())?;
        let username = if credential.UserName.is_null() {
            return Err("The saved credential has no username".to_owned());
        } else {
            unsafe { credential.UserName.to_string() }
                .map_err(|_| "The saved credential username is not valid UTF-16".to_owned())?
        };
        let password = decode_password(credential)?;

        Ok(Some(PasswordCredentials { username, password }))
    }

    pub(crate) fn write(&self, credentials: &PasswordCredentials) -> Result<(), String> {
        let mut target = wide_string(&self.target)?;
        let mut username = wide_string(&credentials.username)?;
        let mut password = encode_password(&credentials.password);
        let credential_blob_size = u32::try_from(password.len())
            .map_err(|_| "The password is too large for the system credential store".to_owned())?;
        let credential = CREDENTIALW {
            Type: self.credential_type,
            TargetName: PWSTR(target.as_mut_ptr()),
            CredentialBlobSize: credential_blob_size,
            CredentialBlob: password.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(username.as_mut_ptr()),
            ..Default::default()
        };

        let result = unsafe { CredWriteW(&credential, 0) };
        password.fill(0);
        result.map_err(|error| {
            format!(
                "Signed in, but could not save the password in the system credential store: {error}"
            )
        })
    }

    pub(crate) fn delete(&self) -> Result<(), String> {
        let target = wide_string(&self.target)?;
        match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), self.credential_type, None) } {
            Ok(()) => Ok(()),
            Err(error) if is_not_found(&error) => Ok(()),
            Err(error) => Err(format!(
                "Signed in, but could not remove the saved password from the system credential store: {error}"
            )),
        }
    }
}

struct CredentialBuffer(*mut CREDENTIALW);

impl CredentialBuffer {
    fn get(&self) -> Option<&CREDENTIALW> {
        unsafe { self.0.as_ref() }
    }
}

impl Drop for CredentialBuffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CredFree(self.0.cast()) };
        }
    }
}

fn is_wine() -> bool {
    let Ok(ntdll) = (unsafe { GetModuleHandleA(pcstr(NTDLL)) }) else {
        return false;
    };
    unsafe { GetProcAddress(ntdll, pcstr(WINE_GET_VERSION)) }.is_some()
}

fn pcstr(value: &CStr) -> PCSTR {
    PCSTR(value.as_ptr().cast())
}

fn wide_string(value: &str) -> Result<Vec<u16>, String> {
    if value.contains('\0') {
        return Err("Credential fields must not contain NUL characters".to_owned());
    }

    Ok(value.encode_utf16().chain([0]).collect())
}

fn encode_password(password: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(password.len() * 2);
    for unit in password.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn decode_password(credential: &CREDENTIALW) -> Result<String, String> {
    let size = credential.CredentialBlobSize as usize;
    if !size.is_multiple_of(2) || (size != 0 && credential.CredentialBlob.is_null()) {
        return Err("The saved password has an invalid UTF-16 payload".to_owned());
    }
    if size == 0 {
        return Ok(String::new());
    }

    let bytes = unsafe { std::slice::from_raw_parts(credential.CredentialBlob, size) };
    let mut units = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| u16::from_le_bytes(*bytes))
        .collect::<Vec<_>>();
    let password =
        String::from_utf16(&units).map_err(|_| "The saved password is not valid UTF-16".to_owned());
    units.fill(0);
    password
}

fn is_not_found(error: &windows::core::Error) -> bool {
    error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_name_scopes_the_account_to_the_sign_service() {
        let store = CredentialStore::new(" http://127.0.0.1:53313/ ");

        assert_eq!(store.target, "Shrimpman MHF — http://127.0.0.1:53313");
    }

    #[test]
    fn password_blob_is_utf16_little_endian() {
        let mut blob = encode_password("猎人🔑");
        let credential = CREDENTIALW {
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            ..Default::default()
        };

        assert_eq!(decode_password(&credential).unwrap(), "猎人🔑");
    }
}
