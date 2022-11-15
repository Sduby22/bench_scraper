#![warn(missing_docs)]
// https://textslashplain.com/2020/09/28/local-data-encryption-in-chromium/
use cbc::cipher::BlockDecryptMut;
use cbc::cipher::KeyIvInit;
use pbkdf2::password_hash::PasswordHasher;

use crate::Error;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

// https://source.chromium.org/chromium/chromium/src/+/main:components/os_crypt/os_crypt_linux.cc
#[cfg(target_os = "linux")]
const DEFAULT_CHROMIUM_SECRET: &[u8] = b"peanuts";
// https://source.chromium.org/chromium/chromium/src/+/main:components/os_crypt/os_crypt_linux.cc
#[cfg(target_os = "linux")]
const CHROMIUM_SALT: &[u8] = b"saltysalt";
// https://source.chromium.org/chromium/chromium/src/+/main:components/os_crypt/os_crypt_mac.mm
#[cfg(target_os = "macos")]
const CHROMIUM_SALT: &[u8] = b"saltysalt";

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub type ChromiumKey = Vec<u8>;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub type ChromiumKeyRef<'a> = &'a [u8];
#[cfg(target_os = "windows")]
pub type ChromiumKey = Option<Vec<[u8]>>;
#[cfg(target_os = "windows")]
pub type ChromiumKeyRef = &Option<Vec<[u8]>>;

fn decrypt_aes128cbc_value(
    key: ChromiumKeyRef,
    value: &[u8],
) -> Result<Vec<u8>, block_padding::UnpadError> {
    // https://gist.github.com/creachadair/937179894a24571ce9860e2475a2d2ec
    let iv = [32u8; 16];
    let dec = Aes128CbcDec::new(key.into(), &iv.into());
    let mut v: Vec<u8> = value.to_vec();
    let slice = dec.decrypt_padded_mut::<block_padding::Pkcs7>(&mut v)?;
    Ok(slice.to_vec())
}

#[cfg(target_os = "windows")]
fn dpapi_crypt_unprotected_data(data: &[u8]) -> Result<Vec<u8>, ()> {
    // TODO: wtf is this
    /*
    let mut null = std::ptr::null_mut(),
    let mut c_uchar_array: [winapi::shared::minwindef::BYTE; 4096] = [0; 4096];
    let mut blob = winapi::um::wincrypt::CRYPTOAPI_BLOB {
        cbData: 0,
        pbData: c_uchar_array,
    };
    winapi::um::dpapi::CryptUnprotectData(data.as_mut_ptr(), null, null, null, null, null, blob);
     */
    todo!();
}

/// Loads the key used to encrypt Chromium browser cookies.
///
/// The name should match the name that the browser uses when talking to the key storage system.
/// See [KnownStorageNames] for a set of known storage names that can be used with this function.
#[cfg(target_os = "linux")]
pub fn get_chromium_master_key(name: &str) -> Result<ChromiumKey, Error> {
    let ss = secret_service::SecretService::new(secret_service::EncryptionType::Plain)?;
    let items = ss.search_items(vec![("Label", name)])?;
    let secret: Vec<u8> = items
        .first()
        .map_or_else(|| Ok(DEFAULT_CHROMIUM_SECRET.to_vec()), |i| i.get_secret())?;
    let salt = pbkdf2::password_hash::SaltString::b64_encode(CHROMIUM_SALT)?;
    let hash = pbkdf2::Pbkdf2.hash_password_customized(
        &secret,
        Some(pbkdf2::Algorithm::Pbkdf2Sha1.ident()),
        None,
        pbkdf2::Params {
            rounds: 1,
            output_length: 16,
        },
        salt.as_salt(),
    )?;
    Ok(hash.hash.unwrap().as_bytes().to_vec())
}
#[cfg(target_os = "macos")]
pub fn get_chromium_master_key(name: &str) -> Result<ChromiumKey, Error> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-wa", name])
        .output()?;
    output.exit_status.exit_ok()?;
    let secret = str::from_utf8(output.stdout).trim().as_bytes();
    let salt = pbkdf2::password_hash::SaltString::b64_encode(CHROMIUM_SALT)?;
    let hash = pbkdf2::Pbkdf2.hash_password_customized(
        &secret,
        Some(pbkdf2::Algorithm::Pbkdf2Sha1.ident()),
        None,
        pbkdf2::Params {
            rounds: 1,
            output_length: 16,
        },
        salt.as_salt(),
    )?;
    Ok(hash.hash.unwrap().as_bytes().to_vec())
}
#[cfg(target_os = "windows")]
pub fn get_chromium_master_key(_name: &str) -> Result<ChromiumKey, Error> {
    // TODO:
    // figure out the path to "Local State" in chromium
    // load the os_crypt.encrypted_key data from "Local State"
    // decode that as base64
    // pass that slice [5:] to DPAPI to get the key
    todo!();
}

/// Decrypts a Chromium cookie pulled from the cookie database.
///
/// The encryption key can be obtained by calling [get_chromium_master_key].
///
/// If decryption fails for any reason (including a non-utf8 decrypted value), `None` will be returned.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn decrypt_chromium_cookie_value(key: ChromiumKeyRef, value: &[u8]) -> Option<String> {
    decrypt_aes128cbc_value(key, &value[3..])
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
}
#[cfg(target_os = "windows")]
pub fn decrypt_chromium_cookie_value(key: ChromiumKeyRef, value: &[u8]) -> Option<String> {
    match key {
        Some(k) => todo!(),
        None => String::from_utf8(dpapi_crypt_unprotected_data(value)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decrypt_aes128cbc_value() {
        let key: Vec<u8> = [
            253, 98, 31, 229, 162, 180, 2, 83, 157, 250, 20, 124, 169, 39, 39, 120,
        ]
        .to_vec();
        let val1: Vec<u8> = [
            148, 145, 230, 74, 69, 235, 97, 242, 23, 248, 235, 32, 190, 142, 2, 210, 13, 42, 117,
            99, 191, 120, 13, 176, 129, 146, 170, 43, 67, 90, 49, 104, 122, 47, 227, 64, 76, 208,
            153, 237, 112, 4, 249, 189, 123, 115, 138, 119, 206, 220, 151, 97, 159, 27, 46, 177,
            167, 28, 69, 124, 116, 190, 149, 227, 122, 119, 86, 189, 135, 167, 228, 192, 38, 57,
            145, 172, 129, 82, 8, 19, 176, 193, 106, 99, 174, 78, 111, 85, 205, 57, 112, 246, 25,
            54, 238, 83,
        ]
        .to_vec();
        let res1: Vec<u8> = [
            115, 37, 51, 65, 72, 107, 85, 67, 48, 111, 119, 95, 86, 105, 55, 69, 106, 102, 112, 87,
            87, 105, 74, 52, 112, 72, 121, 110, 78, 109, 54, 49, 83, 117, 77, 90, 46, 83, 51, 108,
            72, 85, 85, 102, 78, 37, 50, 66, 122, 70, 101, 54, 86, 108, 37, 50, 70, 50, 119, 89,
            103, 75, 111, 65, 75, 72, 107, 109, 118, 109, 57, 51, 76, 111, 88, 51, 105, 116, 75,
            121, 109, 86, 109, 115,
        ]
        .to_vec();
        let r1 = decrypt_aes128cbc_value(&key, &val1).unwrap();
        assert_eq!(r1, res1);
        let val2: Vec<u8> = [
            71, 166, 243, 159, 53, 216, 173, 206, 11, 134, 237, 189, 224, 73, 209, 101,
        ]
        .to_vec();
        let res2: Vec<u8> = [53, 53, 54, 53, 48, 55, 50, 56].to_vec();
        let r2 = decrypt_aes128cbc_value(&key, &val2).unwrap();
        assert_eq!(r2, res2);
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn test_decrypt_chromium_cookie_value() {
        let key: Vec<u8> = [
            253, 98, 31, 229, 162, 180, 2, 83, 157, 250, 20, 124, 169, 39, 39, 120,
        ]
        .to_vec();
        let val: Vec<u8> = [
            19, 0, 250, 71, 166, 243, 159, 53, 216, 173, 206, 11, 134, 237, 189, 224, 73, 209, 101,
        ]
        .to_vec();
        let res = "55650728";
        let r = decrypt_chromium_cookie_value(&key, &val).unwrap();
        assert_eq!(r, res);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_decrypt_chromium_cookie_value() {
        let key: Vec<u8> = [
            253, 98, 31, 229, 162, 180, 2, 83, 157, 250, 20, 124, 169, 39, 39, 120,
        ]
        .to_vec();
        let val: Vec<u8> = [
            19, 0, 250, 71, 166, 243, 159, 53, 216, 173, 206, 11, 134, 237, 189, 224, 73, 209, 101,
        ]
        .to_vec();
        let res = "55650728";
        let r = decrypt_chromium_cookie_value(&key, &val).unwrap();
        assert_eq!(r, res);
    }
}
