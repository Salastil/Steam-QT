//! User settings and stored credentials (XDG locations).

use crate::library::LibraryFolder;
use crate::session::Credentials;
use std::path::{Path, PathBuf};

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback)
        })
        .join("steam-qt")
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share")
}

pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache")
}

/// Suggested location for a first library when no other is configured.
pub fn default_library_path() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Games/SteamLibrary")
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub libraries: Vec<LibraryFolder>,
    /// Index into `libraries` used for new installs.
    pub default_library: usize,
    /// Compatibility tool for Windows-only games, by Steam app id or tool directory name.
    pub default_proton: String,
    pub auto_update: bool,
    /// Download limit in KiB/s; 0 is unlimited.
    pub bandwidth_limit_kib: u32,
    pub language: String,
    pub remember_account: Option<String>,
    /// Per-app runtime choice ("native"/"proton") for apps not yet installed, keyed by app id
    /// (string keys: TOML tables cannot have integer keys).
    pub runtime_choice: std::collections::BTreeMap<String, String>,
    /// Per-app compatibility tool key (see `launch::CompatTool::key`), keyed by app id.
    pub proton_choice: std::collections::BTreeMap<String, String>,
    /// Per-app Steam-style launch options (`ENV=1 %command% -args`), keyed by app id.
    pub launch_options: std::collections::BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            libraries: Vec::new(),
            default_library: 0,
            default_proton: "1493710".into(),
            auto_update: true,
            bandwidth_limit_kib: 0,
            language: "english".into(),
            remember_account: None,
            runtime_choice: Default::default(),
            proton_choice: Default::default(),
            launch_options: Default::default(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        config_dir().join("settings.toml")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> Self {
        let mut settings: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default();
        // Tool keys are Steam app ids or `custom:<dir>`; anything else falls back to the default.
        let key = &settings.default_proton;
        if key.parse::<u32>().is_err() && !key.starts_with("custom:") {
            settings.default_proton = Self::default().default_proton;
        }
        settings
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }

    pub fn default_library(&self) -> Option<&LibraryFolder> {
        self.libraries
            .get(self.default_library)
            .or_else(|| self.libraries.first())
    }

    /// Add a folder (no-op if present). Returns its index.
    pub fn add_library(&mut self, path: impl Into<PathBuf>) -> usize {
        let path = path.into();
        if let Some(i) = self.libraries.iter().position(|l| l.path == path) {
            return i;
        }
        self.libraries.push(LibraryFolder::new(path));
        self.libraries.len() - 1
    }

    /// Remove a folder from the list (files are left alone).
    pub fn remove_library(&mut self, index: usize) {
        if index >= self.libraries.len() {
            return;
        }
        self.libraries.remove(index);
        if self.default_library >= self.libraries.len() || self.default_library == index {
            self.default_library = 0;
        } else if self.default_library > index {
            self.default_library -= 1;
        }
    }

    pub fn set_default_library(&mut self, index: usize) {
        if index < self.libraries.len() {
            self.default_library = index;
        }
    }
}

/// Refresh-token storage for "remember me".
///
/// Stored in the desktop keyring (KWallet / GNOME Keyring via the Secret Service API) when
/// available, otherwise in a mode-0600 file under the data directory.
pub mod credentials {
    use super::*;

    const SERVICE: &str = "steam-qt";

    fn file(account: &str) -> PathBuf {
        let safe: String = account
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        data_dir().join("accounts").join(format!("{safe}.token"))
    }

    /// The keyring's Secret Service backend drives its own async runtime and blocks on it,
    /// which panics when called from inside a tokio runtime (the app's worker). Run keyring
    /// calls on a plain thread so they work from any context.
    fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        std::thread::spawn(f).join().ok()
    }

    fn keyring_set(account: &str, token: &str) -> bool {
        let (account, token) = (account.to_string(), token.to_string());
        off_runtime(move || {
            keyring::Entry::new(SERVICE, &account)
                .and_then(|e| e.set_password(&token))
                .is_ok()
        })
        .unwrap_or(false)
    }

    fn keyring_get(account: &str) -> Option<String> {
        let account = account.to_string();
        off_runtime(move || {
            keyring::Entry::new(SERVICE, &account)
                .and_then(|e| e.get_password())
                .ok()
        })
        .flatten()
    }

    fn keyring_delete(account: &str) {
        let account = account.to_string();
        off_runtime(move || {
            if let Ok(entry) = keyring::Entry::new(SERVICE, &account) {
                let _ = entry.delete_credential();
            }
        });
    }

    pub fn save(creds: &Credentials) -> std::io::Result<()> {
        if keyring_set(&creds.account, &creds.refresh_token) {
            let _ = std::fs::remove_file(file(&creds.account));
            return Ok(());
        }
        let path = file(&creds.account);
        std::fs::create_dir_all(path.parent().expect("has parent"))?;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)?;
        f.write_all(creds.refresh_token.as_bytes())
    }

    pub fn load(account: &str) -> Option<Credentials> {
        let token = keyring_get(account)
            .or_else(|| std::fs::read_to_string(file(account)).ok())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())?;
        Some(Credentials {
            account: account.to_string(),
            refresh_token: token,
        })
    }

    pub fn forget(account: &str) {
        keyring_delete(account);
        let _ = std::fs::remove_file(file(account));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_default_folder() {
        let path =
            std::env::temp_dir().join(format!("steamqt-settings-{}.toml", std::process::id()));
        let mut s = Settings::default();
        assert!(s.default_library().is_none());
        s.add_library("/games/a");
        s.add_library("/games/b");
        assert_eq!(s.add_library("/games/a"), 0);
        s.set_default_library(1);
        assert_eq!(s.default_library().unwrap().path, PathBuf::from("/games/b"));
        s.save_to(&path).unwrap();
        let loaded = Settings::load_from(&path);
        assert_eq!(loaded, s);
        let mut s = loaded;
        s.remove_library(0);
        assert_eq!(s.default_library, 0);
        assert_eq!(s.default_library().unwrap().path, PathBuf::from("/games/b"));
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod keyring_tests {
    use super::*;

    /// Regression test: credential calls from inside a tokio runtime must not panic
    /// (the keyring backend blocks on its own runtime).
    #[tokio::test(flavor = "multi_thread")]
    async fn credentials_usable_inside_runtime() {
        let dir = std::env::temp_dir().join(format!("steamqt-cred-{}", std::process::id()));
        // SAFETY: test-local override of the data directory.
        unsafe { std::env::set_var("XDG_DATA_HOME", &dir) };
        let creds = Credentials {
            account: "probe_user".into(),
            refresh_token: "token123".into(),
        };
        credentials::save(&creds).unwrap();
        assert_eq!(
            credentials::load("probe_user")
                .map(|c| c.refresh_token)
                .as_deref(),
            Some("token123")
        );
        credentials::forget("probe_user");
        assert!(credentials::load("probe_user").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
