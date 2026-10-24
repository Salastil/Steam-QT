//! Typed views over PICS (Product Info Cache Server) app and package data.
//!
//! Field names follow the appinfo layout documented by SteamKit2/SteamDB and observed in the
//! decompiled client (`decomp/linux64_steamclient.so`, `CAppInfoCache`).

use std::collections::BTreeMap;
use steamroom::types::{KeyValue, KvValue};

pub use steamroom::types::key_value::{parse_binary_kv, parse_text_kv};

/// Parse an app info buffer (text VDF, occasionally binary KV).
pub fn parse_app_buffer(data: &[u8]) -> Option<KeyValue> {
    if data.first() == Some(&0x00) {
        return parse_binary_kv(data).ok();
    }
    let text = String::from_utf8_lossy(data);
    parse_text_kv(text.trim_end_matches('\0')).ok()
}

/// Parse a package info buffer: a 4-byte package id followed by binary KV.
pub fn parse_package_buffer(data: &[u8]) -> Option<KeyValue> {
    let body = if data.len() > 4 && data[0] != 0x00 {
        &data[4..]
    } else {
        data
    };
    parse_binary_kv(body).ok()
}

fn kv_string(kv: &KeyValue) -> Option<String> {
    match &kv.value {
        KvValue::String(s) => Some(s.clone()),
        KvValue::Int32(v) => Some(v.to_string()),
        KvValue::UInt64(v) => Some(v.to_string()),
        KvValue::Int64(v) => Some(v.to_string()),
        _ => None,
    }
}

fn kv_u64(kv: &KeyValue) -> Option<u64> {
    match &kv.value {
        KvValue::String(s) => s.trim().parse().ok(),
        KvValue::Int32(v) => u64::try_from(*v).ok(),
        KvValue::UInt64(v) => Some(*v),
        KvValue::Int64(v) => u64::try_from(*v).ok(),
        _ => None,
    }
}

fn path<'a>(kv: &'a KeyValue, keys: &[&str]) -> Option<&'a KeyValue> {
    keys.iter().try_fold(kv, |node, key| node.get(key))
}

fn str_at(kv: &KeyValue, keys: &[&str]) -> Option<String> {
    path(kv, keys).and_then(kv_string)
}

fn children(kv: Option<&KeyValue>) -> impl Iterator<Item = (&String, &KeyValue)> {
    let map: Option<&BTreeMap<String, KeyValue>> = match kv.map(|k| &k.value) {
        Some(KvValue::Children(map)) => Some(map),
        _ => None,
    };
    map.into_iter().flat_map(|m| m.iter())
}

fn csv(s: Option<String>) -> Vec<String> {
    s.map(|s| {
        s.split(',')
            .map(|p| p.trim().to_ascii_lowercase())
            .filter(|p| !p.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AppType {
    Game,
    Demo,
    Dlc,
    Tool,
    Application,
    Music,
    Config,
    Other,
}

impl AppType {
    fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "game" => Self::Game,
            "demo" => Self::Demo,
            "dlc" => Self::Dlc,
            "tool" => Self::Tool,
            "application" => Self::Application,
            "music" => Self::Music,
            "config" => Self::Config,
            _ => Self::Other,
        }
    }

    /// Whether this type belongs in the user-facing library list.
    pub fn is_library_item(self) -> bool {
        matches!(self, Self::Game | Self::Demo | Self::Application)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DepotManifestRef {
    pub gid: u64,
    pub size: u64,
    pub download: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Depot {
    pub id: u32,
    pub name: Option<String>,
    /// Empty means "all operating systems".
    pub oslist: Vec<String>,
    /// "32", "64" or empty for any.
    pub osarch: Option<String>,
    pub language: Option<String>,
    pub low_violence: bool,
    /// Set when this depot belongs to a DLC; it is only installed if that DLC is owned.
    pub dlc_app_id: Option<u32>,
    /// Content shared from another app's depot (e.g. Steamworks redistributables).
    pub depot_from_app: Option<u32>,
    pub shared_install: bool,
    pub max_size: Option<u64>,
    /// Manifests by branch name.
    pub manifests: BTreeMap<String, DepotManifestRef>,
    /// Branches whose manifest is encrypted (`encryptedmanifests`) and needs a beta password.
    pub encrypted_branches: Vec<String>,
}

impl Depot {
    pub fn supports_os(&self, os: &str) -> bool {
        self.oslist.is_empty() || self.oslist.iter().any(|o| o == os)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Branch {
    pub name: String,
    pub build_id: u32,
    pub time_updated: u64,
    pub description: Option<String>,
    pub password_required: bool,
}

#[derive(Clone, Debug, Default)]
pub struct LaunchEntry {
    pub executable: String,
    pub arguments: Option<String>,
    pub working_dir: Option<String>,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub oslist: Vec<String>,
    pub osarch: Option<String>,
    pub beta_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AppDetails {
    pub app_id: u32,
    pub change_number: u32,
    pub name: String,
    pub kind: AppType,
    pub oslist: Vec<String>,
    pub parent: Option<u32>,
    pub install_dir: Option<String>,
    pub icon: Option<String>,
    pub dlc: Vec<u32>,
    pub depots: Vec<Depot>,
    pub branches: Vec<Branch>,
    pub launch: Vec<LaunchEntry>,
    pub workshop_depot: Option<u32>,
}

impl AppDetails {
    pub fn from_kv(app_id: u32, change_number: u32, root: &KeyValue) -> Self {
        let common = root.get("common");
        let name = str_at(root, &["common", "name"]).unwrap_or_else(|| format!("App {app_id}"));
        let kind = str_at(root, &["common", "type"])
            .map(|t| AppType::parse(&t))
            .unwrap_or(AppType::Other);
        let oslist = csv(str_at(root, &["common", "oslist"]));
        let parent = common
            .and_then(|c| c.get("parent"))
            .and_then(kv_u64)
            .map(|v| v as u32);
        let icon = str_at(root, &["common", "icon"]);
        let install_dir = str_at(root, &["config", "installdir"]);
        let dlc = csv(str_at(root, &["extended", "listofdlc"]))
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();

        let depots_kv = root.get("depots");
        let mut depots = Vec::new();
        for (key, node) in children(depots_kv) {
            let Ok(id) = key.parse::<u32>() else { continue };
            let manifests = children(node.get("manifests"))
                .filter_map(|(branch, m)| {
                    // Older appinfo stores the gid directly as the branch value.
                    let gid = m.get("gid").and_then(kv_u64).or_else(|| kv_u64(m))?;
                    Some((
                        branch.clone(),
                        DepotManifestRef {
                            gid,
                            size: m.get("size").and_then(kv_u64).unwrap_or(0),
                            download: m.get("download").and_then(kv_u64).unwrap_or(0),
                        },
                    ))
                })
                .collect();
            depots.push(Depot {
                id,
                name: node.get("name").and_then(kv_string),
                oslist: csv(str_at(node, &["config", "oslist"])),
                osarch: str_at(node, &["config", "osarch"]).filter(|s| !s.is_empty()),
                language: str_at(node, &["config", "language"]).filter(|s| !s.is_empty()),
                low_violence: str_at(node, &["config", "lowviolence"]).as_deref() == Some("1"),
                dlc_app_id: node.get("dlcappid").and_then(kv_u64).map(|v| v as u32),
                depot_from_app: node.get("depotfromapp").and_then(kv_u64).map(|v| v as u32),
                shared_install: node.get("sharedinstall").and_then(kv_u64) == Some(1),
                max_size: node.get("maxsize").and_then(kv_u64),
                manifests,
                encrypted_branches: children(node.get("encryptedmanifests"))
                    .map(|(b, _)| b.clone())
                    .collect(),
            });
        }

        let branches = children(depots_kv.and_then(|d| d.get("branches")))
            .map(|(name, b)| Branch {
                name: name.clone(),
                build_id: b.get("buildid").and_then(kv_u64).unwrap_or(0) as u32,
                time_updated: b.get("timeupdated").and_then(kv_u64).unwrap_or(0),
                description: b.get("description").and_then(kv_string),
                password_required: b.get("pwdrequired").and_then(kv_u64) == Some(1),
            })
            .collect();

        let launch = children(root.get("config").and_then(|c| c.get("launch")))
            .filter_map(|(_, l)| {
                Some(LaunchEntry {
                    executable: l.get("executable").and_then(kv_string)?,
                    arguments: l.get("arguments").and_then(kv_string),
                    working_dir: l.get("workingdir").and_then(kv_string),
                    description: l.get("description").and_then(kv_string),
                    kind: l.get("type").and_then(kv_string),
                    oslist: csv(str_at(l, &["config", "oslist"])),
                    osarch: str_at(l, &["config", "osarch"]).filter(|s| !s.is_empty()),
                    beta_key: str_at(l, &["config", "betakey"]),
                })
            })
            .collect();

        let workshop_depot = depots_kv
            .and_then(|d| d.get("workshopdepot"))
            .and_then(kv_u64)
            .map(|v| v as u32)
            .filter(|&v| v != 0);

        Self {
            app_id,
            change_number,
            name,
            kind,
            oslist,
            parent,
            install_dir,
            icon,
            dlc,
            depots,
            branches,
            launch,
            workshop_depot,
        }
    }

    pub fn supports_linux(&self) -> bool {
        self.oslist.iter().any(|o| o == "linux")
            || self
                .depots
                .iter()
                .any(|d| d.oslist.iter().any(|o| o == "linux"))
    }

    pub fn supports_windows(&self) -> bool {
        self.oslist.is_empty() || self.oslist.iter().any(|o| o == "windows")
    }

    pub fn branch(&self, name: &str) -> Option<&Branch> {
        self.branches.iter().find(|b| b.name == name)
    }

    /// Small capsule icon on Steam's public CDN.
    pub fn icon_url(&self) -> Option<String> {
        self.icon.as_ref().map(|hash| {
            format!(
                "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/{}/{}.jpg",
                self.app_id, hash
            )
        })
    }

    /// Library header art (460x215), available for nearly every game.
    pub fn header_url(&self) -> String {
        format!(
            "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/header.jpg",
            self.app_id
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct PackageDetails {
    pub package_id: u32,
    pub app_ids: Vec<u32>,
    pub depot_ids: Vec<u32>,
}

impl PackageDetails {
    pub fn from_kv(package_id: u32, root: &KeyValue) -> Self {
        let ids = |key: &str| {
            children(root.get(key))
                .filter_map(|(_, v)| kv_u64(v))
                .map(|v| v as u32)
                .collect()
        };
        Self {
            package_id,
            app_ids: ids("appids"),
            depot_ids: ids("depotids"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#""appinfo"
{
    "appid"		"427520"
    "common"
    {
        "name"		"Factorio"
        "type"		"Game"
        "oslist"		"windows,macos,linux"
        "icon"		"abc123"
    }
    "extended"
    {
        "listofdlc"		"645390, 2235890"
    }
    "config"
    {
        "installdir"		"Factorio"
        "launch"
        {
            "0"
            {
                "executable"		"bin/x64/factorio.exe"
                "config" { "oslist" "windows" "osarch" "64" }
            }
            "1"
            {
                "executable"		"bin/x64/factorio"
                "arguments"		"--fullscreen"
                "config" { "oslist" "linux" }
            }
        }
    }
    "depots"
    {
        "427521"
        {
            "config" { "oslist" "windows" }
            "manifests" { "public" { "gid" "1234567890123" "size" "100" "download" "50" } }
        }
        "427522"
        {
            "config" { "oslist" "linux" }
            "manifests" { "public" { "gid" "42" "size" "200" "download" "80" } }
            "encryptedmanifests" { "experimental" { "gid" "x" } }
        }
        "645391"
        {
            "dlcappid"		"645390"
            "manifests" { "public" { "gid" "7" "size" "1" "download" "1" } }
        }
        "branches"
        {
            "public" { "buildid" "19400112" "timeupdated" "1761200000" }
            "experimental" { "buildid" "19400200" "pwdrequired" "1" }
        }
        "workshopdepot"		"0"
    }
}
"#;

    #[test]
    fn parses_appinfo() {
        let kv = parse_app_buffer(SAMPLE.as_bytes()).expect("parse");
        let app = AppDetails::from_kv(427520, 99, &kv);
        assert_eq!(app.name, "Factorio");
        assert_eq!(app.kind, AppType::Game);
        assert!(app.supports_linux() && app.supports_windows());
        assert_eq!(app.dlc, vec![645390, 2235890]);
        assert_eq!(app.install_dir.as_deref(), Some("Factorio"));
        assert_eq!(app.depots.len(), 3);
        let linux = app.depots.iter().find(|d| d.id == 427522).unwrap();
        assert!(linux.supports_os("linux") && !linux.supports_os("windows"));
        assert_eq!(linux.manifests["public"].gid, 42);
        assert_eq!(linux.encrypted_branches, vec!["experimental".to_string()]);
        let dlc = app.depots.iter().find(|d| d.id == 645391).unwrap();
        assert_eq!(dlc.dlc_app_id, Some(645390));
        assert!(dlc.supports_os("linux"));
        assert_eq!(app.branch("public").unwrap().build_id, 19400112);
        assert!(app.branch("experimental").unwrap().password_required);
        assert_eq!(app.launch.len(), 2);
        assert_eq!(app.workshop_depot, None);
        assert!(app.icon_url().unwrap().ends_with("/427520/abc123.jpg"));
    }
}
