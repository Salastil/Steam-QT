//! Library folders and installed-app manifests.
//!
//! Layout and file formats match the official client, so a folder can be shared between
//! Steam-QT and Steam (or imported from it) without re-downloading anything:
//!
//! ```text
//! <library>/libraryfolder.vdf
//! <library>/steamapps/appmanifest_<appid>.acf
//! <library>/steamapps/common/<installdir>/
//! <library>/steamapps/downloading/<appid>/      staging for in-progress updates
//! <library>/steamapps/compatdata/<appid>/        Proton prefixes
//! <library>/steamapps/shadercache/<appid>/
//! <library>/steamapps/workshop/content/<appid>/
//! ```

use crate::vdf::{self, Node};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `StateFlags` bits from `appmanifest_*.acf` (EAppState in the Steamworks headers).
pub mod state {
    pub const UNINSTALLED: u32 = 1;
    pub const UPDATE_REQUIRED: u32 = 2;
    pub const FULLY_INSTALLED: u32 = 4;
    pub const UPDATE_QUEUED: u32 = 8;
    pub const UPDATE_RUNNING: u32 = 256;
    pub const UPDATE_PAUSED: u32 = 512;
    pub const UPDATE_STARTED: u32 = 1024;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstalledDepot {
    pub manifest: u64,
    pub size: u64,
    pub dlc_app_id: Option<u32>,
}

/// Contents of `appmanifest_<appid>.acf`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppManifest {
    pub app_id: u32,
    pub name: String,
    pub state_flags: u32,
    pub install_dir: String,
    pub last_updated: u64,
    pub size_on_disk: u64,
    pub build_id: u32,
    pub last_owner: u64,
    pub bytes_to_download: u64,
    pub bytes_downloaded: u64,
    pub auto_update_behavior: u32,
    pub installed_depots: BTreeMap<u32, InstalledDepot>,
    pub language: Option<String>,
    pub beta_key: Option<String>,
    /// Steam-QT extension: requested runtime ("native" or "proton"); ignored by Steam.
    pub runtime: Option<String>,
    /// Steam-QT extension: DLC the user chose not to install; ignored by Steam.
    pub disabled_dlc: Vec<u32>,
}

impl AppManifest {
    pub fn is_installed(&self) -> bool {
        self.state_flags & state::FULLY_INSTALLED != 0
    }

    pub fn needs_update(&self) -> bool {
        self.state_flags & (state::UPDATE_REQUIRED | state::UPDATE_QUEUED | state::UPDATE_STARTED)
            != 0
    }

    pub fn parse(text: &str) -> Option<Self> {
        let kv = vdf::parse(text)?;
        let s = |k: &str| vdf::get_str(&kv, k);
        let n = |k: &str| vdf::get_u64(&kv, k).unwrap_or(0);
        let installed_depots = vdf::children(kv.get("InstalledDepots"))
            .into_iter()
            .filter_map(|(id, d)| {
                Some((
                    id.parse().ok()?,
                    InstalledDepot {
                        manifest: vdf::get_u64(d, "manifest").unwrap_or(0),
                        size: vdf::get_u64(d, "size").unwrap_or(0),
                        dlc_app_id: vdf::get_u64(d, "dlcappid").map(|v| v as u32),
                    },
                ))
            })
            .collect();
        let user = kv.get("UserConfig");
        let user_str = |k: &str| {
            user.and_then(|u| vdf::get_str(u, k))
                .filter(|v| !v.is_empty())
        };
        Some(Self {
            app_id: n("appid") as u32,
            name: s("name").unwrap_or_default(),
            state_flags: n("StateFlags") as u32,
            install_dir: s("installdir").unwrap_or_default(),
            last_updated: n("LastUpdated"),
            size_on_disk: n("SizeOnDisk"),
            build_id: n("buildid") as u32,
            last_owner: n("LastOwner"),
            bytes_to_download: n("BytesToDownload"),
            bytes_downloaded: n("BytesDownloaded"),
            auto_update_behavior: n("AutoUpdateBehavior") as u32,
            installed_depots,
            language: user_str("language"),
            beta_key: user_str("BetaKey"),
            runtime: user_str("SteamQtRuntime"),
            disabled_dlc: user_str("SteamQtDisabledDLC")
                .map(|s| s.split(',').filter_map(|v| v.trim().parse().ok()).collect())
                .unwrap_or_default(),
        })
    }

    pub fn to_vdf(&self) -> String {
        let mut root = Node::table();
        root.val("appid", self.app_id)
            .val("universe", 1)
            .val("name", &self.name)
            .val("StateFlags", self.state_flags)
            .val("installdir", &self.install_dir)
            .val("LastUpdated", self.last_updated)
            .val("SizeOnDisk", self.size_on_disk)
            .val("StagingSize", 0)
            .val("buildid", self.build_id)
            .val("LastOwner", self.last_owner)
            .val("BytesToDownload", self.bytes_to_download)
            .val("BytesDownloaded", self.bytes_downloaded)
            .val("BytesToStage", self.bytes_to_download)
            .val("BytesStaged", self.bytes_downloaded)
            .val("AutoUpdateBehavior", self.auto_update_behavior)
            .val("AllowOtherDownloadsWhileRunning", 0)
            .val("ScheduledAutoUpdate", 0);
        let mut depots = Node::table();
        for (id, d) in &self.installed_depots {
            let mut node = Node::table();
            node.val("manifest", d.manifest).val("size", d.size);
            if let Some(dlc) = d.dlc_app_id {
                node.val("dlcappid", dlc);
            }
            depots.set(id.to_string(), node);
        }
        root.set("InstalledDepots", depots);
        let mut user = Node::table();
        user.val("language", self.language.as_deref().unwrap_or("english"));
        if let Some(beta) = &self.beta_key {
            user.val("BetaKey", beta);
        }
        if let Some(rt) = &self.runtime {
            user.val("SteamQtRuntime", rt);
        }
        if !self.disabled_dlc.is_empty() {
            let list: Vec<String> = self.disabled_dlc.iter().map(u32::to_string).collect();
            user.val("SteamQtDisabledDLC", list.join(","));
        }
        root.set("UserConfig", user.clone());
        root.set("MountedConfig", user);
        vdf::to_string("AppState", &root)
    }
}

/// A library folder on disk.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryFolder {
    pub path: PathBuf,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DiskSpace {
    pub total: u64,
    pub free: u64,
}

impl LibraryFolder {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            label: String::new(),
        }
    }

    pub fn steamapps(&self) -> PathBuf {
        self.path.join("steamapps")
    }

    pub fn common(&self) -> PathBuf {
        self.steamapps().join("common")
    }

    pub fn install_path(&self, install_dir: &str) -> PathBuf {
        self.common().join(install_dir)
    }

    pub fn staging(&self, app_id: u32) -> PathBuf {
        self.steamapps()
            .join("downloading")
            .join(app_id.to_string())
    }

    pub fn compatdata(&self, app_id: u32) -> PathBuf {
        self.steamapps().join("compatdata").join(app_id.to_string())
    }

    pub fn workshop_content(&self, app_id: u32) -> PathBuf {
        self.steamapps()
            .join("workshop")
            .join("content")
            .join(app_id.to_string())
    }

    pub fn manifest_path(&self, app_id: u32) -> PathBuf {
        self.steamapps().join(format!("appmanifest_{app_id}.acf"))
    }

    /// Create the standard directory layout and `libraryfolder.vdf` marker.
    pub fn ensure_layout(&self) -> std::io::Result<()> {
        for dir in [
            "common",
            "downloading",
            "compatdata",
            "shadercache",
            "workshop",
        ] {
            std::fs::create_dir_all(self.steamapps().join(dir))?;
        }
        let marker = self.path.join("libraryfolder.vdf");
        if !marker.exists() {
            let mut root = Node::table();
            root.val("contentid", content_id(&self.path))
                .val("label", &self.label);
            std::fs::write(marker, vdf::to_string("libraryfolder", &root))?;
        }
        Ok(())
    }

    pub fn read_manifest(&self, app_id: u32) -> Option<AppManifest> {
        AppManifest::parse(&std::fs::read_to_string(self.manifest_path(app_id)).ok()?)
    }

    /// Write atomically (temp file + rename) so a crash never leaves a truncated manifest.
    pub fn write_manifest(&self, manifest: &AppManifest) -> std::io::Result<()> {
        std::fs::create_dir_all(self.steamapps())?;
        let path = self.manifest_path(manifest.app_id);
        let tmp = path.with_extension("acf.tmp");
        std::fs::write(&tmp, manifest.to_vdf())?;
        std::fs::rename(tmp, path)
    }

    pub fn remove_manifest(&self, app_id: u32) -> std::io::Result<()> {
        match std::fs::remove_file(self.manifest_path(app_id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// All app manifests in this library.
    pub fn manifests(&self) -> Vec<AppManifest> {
        let Ok(entries) = std::fs::read_dir(self.steamapps()) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.starts_with("appmanifest_") && name.ends_with(".acf")
            })
            .filter_map(|e| AppManifest::parse(&std::fs::read_to_string(e.path()).ok()?))
            .collect()
    }

    pub fn disk_space(&self) -> Option<DiskSpace> {
        disk_space(&self.path)
    }
}

/// Stable per-folder id, as Steam writes in `libraryfolder.vdf`.
fn content_id(path: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    h.finish() >> 1
}

/// Free/total space of the filesystem holding `path` (or its nearest existing ancestor).
#[allow(clippy::unnecessary_cast)] // statvfs field widths differ between targets
pub fn disk_space(path: &Path) -> Option<DiskSpace> {
    let existing = path.ancestors().find(|p| p.exists())?;
    let c = std::ffi::CString::new(existing.as_os_str().as_encoded_bytes()).ok()?;
    let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `c` is a valid NUL-terminated path and `st` is a properly sized out-parameter.
    if unsafe { libc::statvfs(c.as_ptr(), st.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statvfs returned success, so the struct is initialised.
    let st = unsafe { st.assume_init() };
    let block = st.f_frsize as u64;
    Some(DiskSpace {
        total: st.f_blocks as u64 * block,
        free: st.f_bavail as u64 * block,
    })
}

/// An official Steam client installation found on this machine.
#[derive(Clone, Debug)]
pub struct ExternalSteam {
    pub root: PathBuf,
    pub flatpak: bool,
    pub libraries: Vec<PathBuf>,
}

/// Locate official Steam installs (native and Flatpak) and the library folders they know.
pub fn detect_official_steam() -> Vec<ExternalSteam> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let candidates = [
        (home.join(".local/share/Steam"), false),
        (home.join(".steam/debian-installation"), false),
        (
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
            true,
        ),
    ];
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for (root, flatpak) in candidates {
        let Ok(canonical) = root.canonicalize() else {
            continue;
        };
        if seen.contains(&canonical) {
            continue;
        }
        seen.push(canonical.clone());
        let mut libraries = parse_libraryfolders(&canonical.join("steamapps/libraryfolders.vdf"));
        if !libraries.iter().any(|l| l == &canonical) && canonical.join("steamapps").is_dir() {
            libraries.insert(0, canonical.clone());
        }
        out.push(ExternalSteam {
            root: canonical,
            flatpak,
            libraries,
        });
    }
    out
}

/// Library paths from Steam's `steamapps/libraryfolders.vdf`.
pub fn parse_libraryfolders(file: &Path) -> Vec<PathBuf> {
    let Some(kv) = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| vdf::parse(&t))
    else {
        return Vec::new();
    };
    vdf::children(Some(&kv))
        .into_iter()
        .filter_map(|(_, entry)| vdf::get_str(entry, "path").map(PathBuf::from))
        .filter(|p| p.join("steamapps").is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACF: &str = r#""AppState"
{
	"appid"		"620"
	"universe"		"1"
	"name"		"Portal 2"
	"StateFlags"		"4"
	"installdir"		"Portal 2"
	"LastUpdated"		"1700000000"
	"SizeOnDisk"		"12884901888"
	"buildid"		"9876543"
	"LastOwner"		"76561197960287930"
	"BytesToDownload"		"0"
	"BytesDownloaded"		"0"
	"AutoUpdateBehavior"		"0"
	"InstalledDepots"
	{
		"621"
		{
			"manifest"		"8823745682710397384"
			"size"		"12884901888"
		}
		"659"
		{
			"manifest"		"123"
			"size"		"5"
			"dlcappid"		"651"
		}
	}
	"UserConfig"
	{
		"language"		"english"
		"BetaKey"		"beta"
	}
}
"#;

    #[test]
    fn parses_steam_acf() {
        let m = AppManifest::parse(ACF).unwrap();
        assert_eq!(m.app_id, 620);
        assert!(m.is_installed() && !m.needs_update());
        assert_eq!(m.installed_depots[&621].manifest, 8823745682710397384);
        assert_eq!(m.installed_depots[&659].dlc_app_id, Some(651));
        assert_eq!(m.beta_key.as_deref(), Some("beta"));
        assert_eq!(m.last_owner, 76561197960287930);
    }

    #[test]
    fn manifest_round_trip_on_disk() {
        let dir = std::env::temp_dir().join(format!("steamqt-lib-{}", std::process::id()));
        let lib = LibraryFolder::new(&dir);
        lib.ensure_layout().unwrap();
        let mut m = AppManifest::parse(ACF).unwrap();
        m.runtime = Some("proton".into());
        m.disabled_dlc = vec![651, 652];
        lib.write_manifest(&m).unwrap();
        assert_eq!(lib.read_manifest(620), Some(m.clone()));
        assert_eq!(lib.manifests().len(), 1);
        assert!(dir.join("libraryfolder.vdf").exists());
        assert!(lib.disk_space().unwrap().total > 0);
        lib.remove_manifest(620).unwrap();
        assert!(lib.manifests().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_libraryfolders_vdf() {
        let dir = std::env::temp_dir().join(format!("steamqt-lf-{}", std::process::id()));
        let other = dir.join("other");
        std::fs::create_dir_all(other.join("steamapps")).unwrap();
        std::fs::create_dir_all(dir.join("steamapps")).unwrap();
        let text = format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"/nonexistent\"\n\t}}\n}}\n",
            other.display()
        );
        std::fs::write(dir.join("steamapps/libraryfolders.vdf"), text).unwrap();
        assert_eq!(
            parse_libraryfolders(&dir.join("steamapps/libraryfolders.vdf")),
            vec![other]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
