// The Motherlode API key, kept in the OS keychain (Windows Credential Manager,
// macOS Keychain, Secret Service on Linux). Never written to a config file.

const SERVICE: &str = "com.motherlode.uplink";
const ACCOUNT: &str = "api-key";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| format!("Keychain unavailable: {e}"))
}

pub fn load_api_key() -> Option<String> {
    match entry().ok()?.get_password() {
        Ok(k) if !k.is_empty() => Some(k),
        Ok(_) | Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            log::warn!("could not read the API key from the keychain: {e}");
            None
        }
    }
}

pub fn store_api_key(key: &str) -> Result<(), String> {
    entry()?.set_password(key).map_err(|e| format!("Could not save the key to the keychain: {e}"))
}

pub fn delete_api_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Could not remove the key from the keychain: {e}")),
    }
}

/// "mlk_abcd…wxyz" — what the settings screen shows.
pub fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 12 {
        return "mlk_••••".into();
    }
    let head: String = chars[..8].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}••••{tail}")
}
