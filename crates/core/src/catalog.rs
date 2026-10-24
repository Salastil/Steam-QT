//! The user's library: owned apps joined with what is installed in each library folder.

use crate::config::{Settings, cache_dir};
use crate::install;
use crate::library::AppManifest;
use crate::pics::{self, AppDetails, AppType};
use crate::session::{Result, Session};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use tracing::{debug, info};

#[derive(Clone, Debug)]
pub struct Installed {
    /// Index into `Settings::libraries`.
    pub library: usize,
    pub manifest: AppManifest,
    pub update_available: bool,
}

#[derive(Clone, Debug)]
pub struct Dlc {
    pub app_id: u32,
    pub name: String,
    /// Whether this DLC ships depots (content to download) rather than only unlocking.
    pub has_content: bool,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub app: AppDetails,
    pub install: Option<Installed>,
    /// Owned DLC for this app.
    pub dlc: Vec<Dlc>,
}

impl Entry {
    pub fn app_id(&self) -> u32 {
        self.app.app_id
    }
}

#[derive(Default)]
pub struct Catalog {
    /// Library items (games, demos, applications), sorted by name.
    pub entries: Vec<Entry>,
    /// Every app id the account owns, including DLC and tools.
    pub owned: HashSet<u32>,
}

/// Installed apps across all configured library folders, by app id.
pub fn scan_installed(settings: &Settings) -> HashMap<u32, (usize, AppManifest)> {
    let mut out = HashMap::new();
    for (index, lib) in settings.libraries.iter().enumerate() {
        for manifest in lib.manifests() {
            out.entry(manifest.app_id).or_insert((index, manifest));
        }
    }
    out
}

/// On-disk cache of PICS app buffers, keyed by change number.
struct AppInfoCache {
    dir: PathBuf,
}

impl AppInfoCache {
    fn new() -> Self {
        Self {
            dir: cache_dir().join("appinfo"),
        }
    }

    fn path(&self, app_id: u32) -> PathBuf {
        self.dir.join(format!("{app_id}.vdf"))
    }

    fn load(&self, app_id: u32, change_number: u32) -> Option<AppDetails> {
        let data = std::fs::read(self.path(app_id)).ok()?;
        let (header, body) = data.split_at(data.iter().position(|&b| b == b'\n')? + 1);
        let cached: u32 = std::str::from_utf8(header).ok()?.trim().parse().ok()?;
        if cached != change_number {
            return None;
        }
        let kv = pics::parse_app_buffer(body)?;
        Some(AppDetails::from_kv(app_id, change_number, &kv))
    }

    fn store(&self, app_id: u32, change_number: u32, buffer: &[u8]) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let mut data = format!("{change_number}\n").into_bytes();
        data.extend_from_slice(buffer);
        let _ = std::fs::write(self.path(app_id), data);
    }
}

/// Fetch app details, reusing cached info whose change number is current.
pub async fn fetch_apps(session: &Session, app_ids: &[u32]) -> Result<Vec<AppDetails>> {
    let cache = AppInfoCache::new();
    let meta = session.app_buffers(app_ids, true).await?;
    let mut out = Vec::with_capacity(meta.len());
    let mut stale = Vec::new();
    for m in &meta {
        match cache.load(m.app_id, m.change_number) {
            Some(app) => out.push(app),
            None => stale.push(m.app_id),
        }
    }
    debug!(cached = out.len(), fetching = stale.len(), "app info");
    for raw in session.app_buffers(&stale, false).await? {
        let Some(buffer) = raw.buffer else { continue };
        if let Some(kv) = pics::parse_app_buffer(&buffer) {
            cache.store(raw.app_id, raw.change_number, &buffer);
            out.push(AppDetails::from_kv(raw.app_id, raw.change_number, &kv));
        }
    }
    Ok(out)
}

impl Catalog {
    pub async fn load(session: &Session, settings: &Settings) -> Result<Self> {
        let licenses = session.licenses().await;
        let packages: Vec<(u32, u64)> = licenses
            .iter()
            .map(|l| (l.package_id, l.access_token))
            .collect();
        let mut owned: HashSet<u32> = HashSet::new();
        for package in session.packages(&packages).await? {
            owned.extend(package.app_ids);
        }

        let installed = scan_installed(settings);
        let mut wanted: Vec<u32> = owned.iter().chain(installed.keys()).copied().collect();
        wanted.sort_unstable();
        wanted.dedup();
        let apps = fetch_apps(session, &wanted).await?;
        info!(
            licences = licenses.len(),
            owned = owned.len(),
            apps = apps.len(),
            "library loaded"
        );
        Ok(Self::assemble(apps, owned, installed))
    }

    /// Build entries from fetched app details (separate from `load` so it can be tested).
    pub fn assemble(
        apps: Vec<AppDetails>,
        owned: HashSet<u32>,
        mut installed: HashMap<u32, (usize, AppManifest)>,
    ) -> Self {
        let mut dlc_by_parent: BTreeMap<u32, Vec<Dlc>> = BTreeMap::new();
        let mut items = Vec::new();
        for app in apps {
            match app.kind {
                AppType::Dlc if owned.contains(&app.app_id) => {
                    if let Some(parent) = app.parent {
                        dlc_by_parent.entry(parent).or_default().push(Dlc {
                            app_id: app.app_id,
                            name: app.name.clone(),
                            has_content: !app.depots.is_empty(),
                        });
                    }
                }
                kind if kind.is_library_item() => items.push(app),
                _ => {}
            }
        }
        let mut entries: Vec<Entry> = items
            .into_iter()
            .map(|app| {
                let install = installed
                    .remove(&app.app_id)
                    .map(|(library, manifest)| Installed {
                        library,
                        update_available: install::needs_update(&app, &manifest),
                        manifest,
                    });
                let mut dlc = dlc_by_parent.remove(&app.app_id).unwrap_or_default();
                dlc.sort_by(|a, b| a.name.cmp(&b.name));
                Entry { app, install, dlc }
            })
            .collect();
        entries.sort_by_key(|e| e.app.name.to_lowercase());
        Self { entries, owned }
    }

    pub fn get(&self, app_id: u32) -> Option<&Entry> {
        self.entries.iter().find(|e| e.app.app_id == app_id)
    }

    pub fn get_mut(&mut self, app_id: u32) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.app.app_id == app_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pics::parse_app_buffer;

    fn app(id: u32, name: &str, kind: &str, extra: &str) -> AppDetails {
        let text = format!(
            "\"appinfo\" {{ \"common\" {{ \"name\" \"{name}\" \"type\" \"{kind}\" {extra} }} }}"
        );
        AppDetails::from_kv(id, 1, &parse_app_buffer(text.as_bytes()).unwrap())
    }

    #[test]
    fn assembles_entries_with_dlc_and_install_state() {
        let apps = vec![
            app(20, "zeta", "Game", ""),
            app(10, "Alpha", "Game", ""),
            app(11, "Alpha Soundtrack", "DLC", "\"parent\" \"10\""),
            app(12, "Unowned DLC", "DLC", "\"parent\" \"10\""),
            app(30, "Proton", "Tool", ""),
        ];
        let owned: HashSet<u32> = [10, 11, 20, 30].into();
        let mut installed = HashMap::new();
        installed.insert(
            20,
            (
                1,
                AppManifest {
                    app_id: 20,
                    ..Default::default()
                },
            ),
        );
        let cat = Catalog::assemble(apps, owned, installed);
        let names: Vec<_> = cat.entries.iter().map(|e| e.app.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "zeta"], "sorted, tools and DLC excluded");
        assert_eq!(cat.get(10).unwrap().dlc.len(), 1);
        assert_eq!(cat.get(10).unwrap().dlc[0].name, "Alpha Soundtrack");
        assert_eq!(cat.get(20).unwrap().install.as_ref().unwrap().library, 1);
        assert!(cat.get(10).unwrap().install.is_none());
    }
}
