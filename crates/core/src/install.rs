//! Installing, updating, verifying and uninstalling apps.
//!
//! Depot selection follows DepotDownloader/SteamKit2 (`DepotDownloader/ContentDownloader.cs`):
//! a depot is installed when its `oslist`, `osarch` and `language` match the target and, for
//! DLC depots, the DLC is owned. The per-depot download itself is steamroom-client's
//! `DepotJob` (chunk fetch, decrypt, decompress, delta reuse of unchanged chunks); the
//! orchestration around it is adapted from `steamroom-cli`'s download command (MIT).

use crate::library::{AppManifest, InstalledDepot, LibraryFolder, state};
use crate::pics::{AppDetails, Depot, DepotManifestRef};
use crate::session::{Session, SessionError};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use steamroom::cdn::{CdnClient, CdnServerPool};
use steamroom::depot::manifest::DepotManifest;
use steamroom::depot::{DepotId, DepotKey, ManifestId};
use steamroom_client::download::{CdnChunkFetcher, DepotJob, OldChunkLoc};
use steamroom_client::event::DownloadEvent;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Runtime {
    Native,
    Proton,
}

impl Runtime {
    pub fn target_os(self) -> &'static str {
        match self {
            Runtime::Native => "linux",
            Runtime::Proton => "windows",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Runtime::Native => "native",
            Runtime::Proton => "proton",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "native" => Some(Runtime::Native),
            "proton" => Some(Runtime::Proton),
            _ => None,
        }
    }

    /// The runtime an app should use when the user hasn't chosen: native if available.
    pub fn preferred_for(app: &AppDetails) -> Self {
        if app.supports_linux() {
            Runtime::Native
        } else {
            Runtime::Proton
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("no depots of {0} match {1} — nothing to install")]
    NoDepots(String, &'static str),
    #[error("branch '{0}' requires a beta password")]
    BranchLocked(String),
    #[error("not enough disk space: {needed} bytes needed, {free} free")]
    DiskSpace { needed: u64, free: u64 },
    #[error("{0}")]
    Session(#[from] SessionError),
    #[error("CDN: {0}")]
    Cdn(String),
    #[error("depot {0}: {1}")]
    Depot(u32, String),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("cancelled")]
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct InstallRequest {
    pub app: AppDetails,
    pub library: LibraryFolder,
    pub runtime: Runtime,
    pub branch: String,
    pub language: String,
    /// Every app id the account owns (DLC depots are only installed for owned DLC).
    pub owned: HashSet<u32>,
    /// DLC the user unticked.
    pub disabled_dlc: Vec<u32>,
    /// Re-hash every file instead of trusting existing ones.
    pub verify: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlannedDepot {
    pub depot_id: u32,
    pub manifest: DepotManifestRef,
    pub dlc_app_id: Option<u32>,
}

fn arch_ok(depot: &Depot) -> bool {
    depot.osarch.as_deref().is_none_or(|a| a == "64")
}

/// Choose the depots (and manifests) that make up an install.
pub fn plan(req: &InstallRequest) -> Result<Vec<PlannedDepot>, InstallError> {
    let os = req.runtime.target_os();
    let branch = req.branch.as_str();
    if branch != "public"
        && req.app.branch(branch).is_some_and(|b| b.password_required)
        && req
            .app
            .depots
            .iter()
            .any(|d| d.encrypted_branches.iter().any(|b| b == branch))
    {
        return Err(InstallError::BranchLocked(branch.to_string()));
    }

    let candidates: Vec<&Depot> = req
        .app
        .depots
        .iter()
        .filter(|d| d.supports_os(os))
        .filter(|d| d.language.as_deref().is_none_or(|l| l == req.language))
        .filter(|d| !d.low_violence)
        // Shared redistributable depots belong to another app (e.g. 228980) and are
        // provisioned by Proton / the runtime instead.
        .filter(|d| d.depot_from_app.is_none() && !d.shared_install)
        .filter(|d| match d.dlc_app_id {
            Some(dlc) => req.owned.contains(&dlc) && !req.disabled_dlc.contains(&dlc),
            None => true,
        })
        .collect();

    // Prefer 64-bit depots; fall back to 32-bit-only content when that's all there is.
    let has_64 = candidates
        .iter()
        .any(|d| arch_ok(d) && d.dlc_app_id.is_none());
    let mut out = Vec::new();
    for depot in candidates {
        if has_64 && !arch_ok(depot) {
            continue;
        }
        let manifest = depot
            .manifests
            .get(branch)
            .or_else(|| depot.manifests.get("public"));
        let Some(manifest) = manifest else { continue };
        out.push(PlannedDepot {
            depot_id: depot.id,
            manifest: manifest.clone(),
            dlc_app_id: depot.dlc_app_id,
        });
    }
    if out.is_empty() {
        return Err(InstallError::NoDepots(req.app.name.clone(), os));
    }
    Ok(out)
}

/// Progress reported to the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    Preparing,
    Downloading {
        done: u64,
        total: u64,
        bytes_per_sec: u64,
    },
    Finalizing,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn install_dir_name(app: &AppDetails) -> String {
    app.install_dir.clone().unwrap_or_else(|| app.name.clone())
}

fn depotcache(lib: &LibraryFolder) -> PathBuf {
    lib.steamapps().join("depotcache")
}

fn cached_manifest_path(lib: &LibraryFolder, depot: u32, manifest: u64) -> PathBuf {
    depotcache(lib).join(format!("{depot}_{manifest}.manifest"))
}

/// CDN manifests are zip archives holding the protobuf manifest.
fn unzip_manifest(data: &[u8]) -> std::io::Result<Vec<u8>> {
    if data.len() > 2 && data[0] == b'P' && data[1] == b'K' {
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(std::io::Error::other)?;
        let mut file = archive.by_index(0).map_err(std::io::Error::other)?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut buf)?;
        Ok(buf)
    } else {
        Ok(data.to_vec())
    }
}

fn load_cached_manifest(
    lib: &LibraryFolder,
    depot: u32,
    manifest: u64,
    key: &DepotKey,
) -> Option<DepotManifest> {
    let bytes = std::fs::read(cached_manifest_path(lib, depot, manifest)).ok()?;
    let mut m = DepotManifest::parse(&bytes).ok()?;
    if m.filenames_encrypted {
        m.decrypt_filenames(key).ok()?;
    }
    Some(m)
}

fn old_file_layouts(old: &DepotManifest) -> HashMap<String, Vec<OldChunkLoc>> {
    old.files
        .iter()
        .filter(|f| !f.chunks.is_empty())
        .map(|f| {
            let mut running = 0u64;
            let locs = f
                .chunks
                .iter()
                .map(|c| {
                    let offset = c.offset.unwrap_or(running);
                    running = offset + u64::from(c.uncompressed_size);
                    OldChunkLoc {
                        id: c.id.clone(),
                        offset,
                        size: c.uncompressed_size,
                    }
                })
                .collect();
            (f.normalized_path(), locs)
        })
        .collect()
}

/// Aggregates per-depot download events into overall progress.
struct Meter {
    tx: mpsc::UnboundedSender<Progress>,
    total: u64,
    finished_depots: u64,
    current: u64,
    started: Instant,
    last_sent: Instant,
}

impl Meter {
    fn report(&mut self, force: bool) {
        if !force && self.last_sent.elapsed() < Duration::from_millis(250) {
            return;
        }
        self.last_sent = Instant::now();
        let done = self.finished_depots + self.current;
        let secs = self.started.elapsed().as_secs_f64().max(0.5);
        let _ = self.tx.send(Progress::Downloading {
            done,
            total: self.total.max(done),
            bytes_per_sec: (done as f64 / secs) as u64,
        });
    }
}

struct CdnContext {
    cdn: CdnClient,
    servers: Vec<steamroom::cdn::server::CdnServer>,
}

async fn fetch_manifest(
    session: &Session,
    cdn: &CdnContext,
    app_id: u32,
    depot: &PlannedDepot,
    branch: &str,
    key: &DepotKey,
    lib: &LibraryFolder,
) -> Result<DepotManifest, InstallError> {
    if let Some(m) = load_cached_manifest(lib, depot.depot_id, depot.manifest.gid, key) {
        return Ok(m);
    }
    let request_code = session
        .manifest_request_code(app_id, depot.depot_id, depot.manifest.gid, branch)
        .await
        .unwrap_or_else(|e| {
            debug!(depot = depot.depot_id, error = %e, "no manifest request code");
            0
        });
    let pool = CdnServerPool::new(cdn.servers.clone());
    let token = match cdn.servers.first() {
        Some(s) => session
            .cdn_auth_token(app_id, depot.depot_id, &s.host)
            .await
            .ok()
            .flatten(),
        None => None,
    };
    let raw = cdn
        .cdn
        .download_manifest_pooled(
            &pool,
            DepotId(depot.depot_id),
            ManifestId(depot.manifest.gid),
            request_code,
            token.as_deref(),
        )
        .await
        .map_err(|e| InstallError::Cdn(e.to_string()))?;
    let bytes = unzip_manifest(&raw)?;
    std::fs::create_dir_all(depotcache(lib))?;
    std::fs::write(
        cached_manifest_path(lib, depot.depot_id, depot.manifest.gid),
        &bytes,
    )?;
    let mut manifest = DepotManifest::parse(&bytes)
        .map_err(|e| InstallError::Depot(depot.depot_id, e.to_string()))?;
    if manifest.filenames_encrypted {
        manifest
            .decrypt_filenames(key)
            .map_err(|e| InstallError::Depot(depot.depot_id, e.to_string()))?;
    }
    Ok(manifest)
}

/// Install or update an app. Returns the manifest written to the library.
pub async fn install(
    session: &Session,
    req: &InstallRequest,
    progress: mpsc::UnboundedSender<Progress>,
    cancel: CancellationToken,
) -> Result<AppManifest, InstallError> {
    let _ = progress.send(Progress::Preparing);
    let planned = plan(req)?;
    let lib = &req.library;
    lib.ensure_layout()?;
    let app_id = req.app.app_id;
    let install_path = lib.install_path(&install_dir_name(&req.app));

    let previous = lib.read_manifest(app_id);
    let total_download: u64 = planned
        .iter()
        .filter(|p| {
            previous
                .as_ref()
                .and_then(|m| m.installed_depots.get(&p.depot_id))
                .map(|d| d.manifest)
                != Some(p.manifest.gid)
                || req.verify
        })
        .map(|p| p.manifest.size)
        .sum();
    let total_size: u64 = planned.iter().map(|p| p.manifest.size).sum();

    let previous_size = previous.as_ref().map(|m| m.size_on_disk).unwrap_or(0);
    if let Some(space) = lib.disk_space() {
        let needed = total_size.saturating_sub(previous_size);
        if needed > space.free {
            return Err(InstallError::DiskSpace {
                needed,
                free: space.free,
            });
        }
    }

    // Mark the update as in progress, keeping previously installed depots so an interrupted
    // update resumes as a delta.
    let mut manifest = previous.clone().unwrap_or_default();
    manifest.app_id = app_id;
    manifest.name = req.app.name.clone();
    manifest.install_dir = install_dir_name(&req.app);
    manifest.state_flags = state::UPDATE_REQUIRED | state::UPDATE_STARTED;
    manifest.bytes_to_download = total_download;
    manifest.bytes_downloaded = 0;
    manifest.last_owner = session.steam_id();
    manifest.language = Some(req.language.clone());
    manifest.beta_key = (req.branch != "public").then(|| req.branch.clone());
    manifest.runtime = Some(req.runtime.as_str().to_string());
    manifest.disabled_dlc = req.disabled_dlc.clone();
    lib.write_manifest(&manifest)?;

    let cdn = CdnContext {
        cdn: CdnClient::new().map_err(|e| InstallError::Cdn(e.to_string()))?,
        servers: session.cdn_servers().await?,
    };
    if cdn.servers.is_empty() {
        return Err(InstallError::Cdn("no content servers available".into()));
    }

    let mut meter = Meter {
        tx: progress.clone(),
        total: total_download,
        finished_depots: 0,
        current: 0,
        started: Instant::now(),
        last_sent: Instant::now(),
    };
    let mut keep_files: HashSet<String> = HashSet::new();
    let mut new_depots: BTreeMap<u32, InstalledDepot> = BTreeMap::new();

    for depot in &planned {
        if cancel.is_cancelled() {
            return Err(InstallError::Cancelled);
        }
        let key = session.depot_key(app_id, depot.depot_id).await?;
        let new_manifest =
            fetch_manifest(session, &cdn, app_id, depot, &req.branch, &key, lib).await?;
        keep_files.extend(new_manifest.files.iter().map(|f| f.normalized_path()));

        let installed = previous
            .as_ref()
            .and_then(|m| m.installed_depots.get(&depot.depot_id));
        let up_to_date = installed.map(|d| d.manifest) == Some(depot.manifest.gid);
        if up_to_date && !req.verify {
            debug!(depot = depot.depot_id, "already up to date");
            new_depots.insert(depot.depot_id, installed.cloned().unwrap_or_default());
            continue;
        }

        let old = installed
            .filter(|d| d.manifest != depot.manifest.gid)
            .and_then(|d| load_cached_manifest(lib, depot.depot_id, d.manifest, &key));

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let mut builder = DepotJob::builder()
            .depot_id(DepotId(depot.depot_id))
            .depot_key(key.clone())
            .install_dir(install_path.clone())
            .verify(req.verify)
            .event_sender(event_tx);
        if let Some(old) = &old {
            info!(depot = depot.depot_id, "delta update");
            builder = builder
                .old_manifest_files(old.files.iter().map(|f| f.normalized_path()).collect())
                .old_file_layouts(old_file_layouts(old));
        }
        let job = builder
            .build()
            .map_err(|e| InstallError::Depot(depot.depot_id, e.to_string()))?;
        let token = session
            .cdn_auth_token(app_id, depot.depot_id, &cdn.servers[0].host)
            .await
            .ok()
            .flatten();
        let fetcher = Arc::new(CdnChunkFetcher::new(
            CdnClient::new().map_err(|e| InstallError::Cdn(e.to_string()))?,
            CdnServerPool::new(cdn.servers.clone()),
            token,
        ));

        let mut planned_bytes = depot.manifest.size;
        let result = {
            let download = job.download(&new_manifest, fetcher);
            tokio::pin!(download);
            loop {
                tokio::select! {
                    res = &mut download => break Some(res),
                    _ = cancel.cancelled() => break None,
                    Some(event) = event_rx.recv() => {
                        match event {
                            DownloadEvent::DownloadStarted { total_bytes, .. } => {
                                // A delta writes less than the full depot; rescale the total.
                                meter.total = meter.total.saturating_sub(depot.manifest.size) + total_bytes;
                                planned_bytes = total_bytes;
                                meter.report(true);
                            }
                            DownloadEvent::DepotProgress { completed_bytes, .. } => {
                                meter.current = completed_bytes;
                                meter.report(false);
                            }
                            DownloadEvent::ChunkFailed { error } => {
                                warn!(depot = depot.depot_id, error = ?error.messages, "chunk failed, retrying");
                            }
                            _ => {}
                        }
                    }
                }
            }
        };
        drop(job);
        // DepotJob stages files under the install dir; don't leave that behind in the game.
        let _ = std::fs::remove_dir_all(install_path.join(".DepotDownloader"));
        match result {
            None => return Err(InstallError::Cancelled),
            Some(Err(report)) => {
                return Err(InstallError::Depot(depot.depot_id, report.to_string()));
            }
            Some(Ok(stats)) => {
                debug!(
                    depot = depot.depot_id,
                    files = stats.files_completed,
                    bytes = stats.bytes_downloaded,
                    "depot done"
                );
            }
        }
        meter.finished_depots += planned_bytes;
        meter.current = 0;
        meter.report(true);

        new_depots.insert(
            depot.depot_id,
            InstalledDepot {
                manifest: depot.manifest.gid,
                size: depot.manifest.size,
                dlc_app_id: depot.dlc_app_id,
            },
        );
        manifest.bytes_downloaded = meter.finished_depots;
        manifest.installed_depots = previous
            .as_ref()
            .map(|m| m.installed_depots.clone())
            .unwrap_or_default();
        manifest.installed_depots.extend(new_depots.clone());
        lib.write_manifest(&manifest)?;
    }

    let _ = progress.send(Progress::Finalizing);

    // Depots dropped from the install (runtime switched, DLC disabled): delete their files
    // unless a remaining depot still provides them.
    if let Some(prev) = &previous {
        for (depot_id, old) in &prev.installed_depots {
            if new_depots.contains_key(depot_id) {
                continue;
            }
            let Ok(key) = session.depot_key(app_id, *depot_id).await else {
                continue;
            };
            let Some(old_manifest) = load_cached_manifest(lib, *depot_id, old.manifest, &key)
            else {
                continue;
            };
            for file in old_manifest.files.iter().map(|f| f.normalized_path()) {
                if !keep_files.contains(&file) {
                    let path = install_path.join(&file);
                    if path.is_file() {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
            info!(depot = depot_id, "removed depot no longer selected");
        }
        prune_empty_dirs(&install_path);
    }
    prune_depotcache(lib, &new_depots, previous.as_ref());

    manifest.installed_depots = new_depots;
    manifest.state_flags = state::FULLY_INSTALLED;
    manifest.bytes_to_download = 0;
    manifest.bytes_downloaded = 0;
    manifest.size_on_disk = total_size;
    manifest.build_id = req
        .app
        .branch(&req.branch)
        .or_else(|| req.app.branch("public"))
        .map(|b| b.build_id)
        .unwrap_or(0);
    manifest.last_updated = now();
    lib.write_manifest(&manifest)?;
    info!(app_id, name = %req.app.name, "install complete");
    Ok(manifest)
}

/// Remove directories left empty after deleting a depot's files (never the root itself).
fn prune_empty_dirs(dir: &Path) {
    fn walk(dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        let mut empty = true;
        for entry in entries.flatten() {
            let path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if !(is_dir && walk(&path) && std::fs::remove_dir(&path).is_ok()) {
                empty = false;
            }
        }
        empty
    }
    walk(dir);
}

/// Remove cached manifests that are no longer installed (keeps the current ones for deltas).
fn prune_depotcache(
    lib: &LibraryFolder,
    current: &BTreeMap<u32, InstalledDepot>,
    previous: Option<&AppManifest>,
) {
    let Some(prev) = previous else { return };
    for (depot, old) in &prev.installed_depots {
        if current.get(depot).map(|d| d.manifest) != Some(old.manifest) {
            let _ = std::fs::remove_file(cached_manifest_path(lib, *depot, old.manifest));
        }
    }
}

/// Is an installed app behind the latest build of its branch?
pub fn needs_update(app: &AppDetails, installed: &AppManifest) -> bool {
    let branch = installed.beta_key.as_deref().unwrap_or("public");
    let runtime = installed
        .runtime
        .as_deref()
        .and_then(Runtime::parse)
        .unwrap_or_else(|| {
            // Manifests written by the official client: infer from installed depots.
            let linux = app.depots.iter().any(|d| {
                installed.installed_depots.contains_key(&d.id)
                    && d.oslist.iter().any(|o| o == "linux")
            });
            if linux {
                Runtime::Native
            } else {
                Runtime::Proton
            }
        });
    let req = InstallRequest {
        app: app.clone(),
        library: LibraryFolder::new(PathBuf::new()),
        runtime,
        branch: branch.to_string(),
        language: installed
            .language
            .clone()
            .unwrap_or_else(|| "english".into()),
        owned: installed
            .installed_depots
            .values()
            .filter_map(|d| d.dlc_app_id)
            .collect(),
        disabled_dlc: installed.disabled_dlc.clone(),
        verify: false,
    };
    match plan(&req) {
        Ok(planned) => planned.iter().any(|p| {
            installed
                .installed_depots
                .get(&p.depot_id)
                .map(|d| d.manifest)
                != Some(p.manifest.gid)
        }),
        Err(_) => false,
    }
}

/// Remove an installed app: files, manifest, cached depot manifests. The Proton prefix is
/// kept unless `remove_prefix` (it holds save games for many titles).
pub fn uninstall(lib: &LibraryFolder, app_id: u32, remove_prefix: bool) -> std::io::Result<()> {
    let manifest = lib.read_manifest(app_id);
    if let Some(m) = &manifest {
        let dir = lib.install_path(&m.install_dir);
        if !m.install_dir.is_empty() && dir.starts_with(lib.common()) && dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        for (depot, d) in &m.installed_depots {
            let _ = std::fs::remove_file(cached_manifest_path(lib, *depot, d.manifest));
        }
    }
    let _ = std::fs::remove_dir_all(lib.staging(app_id));
    if remove_prefix {
        let _ = std::fs::remove_dir_all(lib.compatdata(app_id));
    }
    lib.remove_manifest(app_id)
}

/// Total size of a directory tree (for imported installs lacking SizeOnDisk).
pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pics::{Branch, parse_app_buffer};

    fn app() -> AppDetails {
        let text = r#""appinfo"
{
    "common" { "name" "Test" "type" "Game" "oslist" "windows,linux" }
    "config" { "installdir" "TestGame" }
    "depots"
    {
        "11" { "config" { "oslist" "windows" "osarch" "64" } "manifests" { "public" { "gid" "1" "size" "100" "download" "10" } } }
        "12" { "config" { "oslist" "windows" "osarch" "32" } "manifests" { "public" { "gid" "2" "size" "100" "download" "10" } } }
        "13" { "config" { "oslist" "linux" } "manifests" { "public" { "gid" "3" "size" "100" "download" "10" } "beta" { "gid" "33" "size" "1" "download" "1" } } }
        "14" { "manifests" { "public" { "gid" "4" "size" "5" "download" "5" } } }
        "15" { "config" { "language" "german" } "manifests" { "public" { "gid" "5" "size" "1" "download" "1" } } }
        "16" { "dlcappid" "900" "manifests" { "public" { "gid" "6" "size" "1" "download" "1" } } }
        "17" { "depotfromapp" "228980" "sharedinstall" "1" }
        "branches" { "public" { "buildid" "100" } "beta" { "buildid" "101" } }
    }
}"#;
        AppDetails::from_kv(10, 1, &parse_app_buffer(text.as_bytes()).unwrap())
    }

    fn req(runtime: Runtime) -> InstallRequest {
        InstallRequest {
            app: app(),
            library: LibraryFolder::new("/tmp/none"),
            runtime,
            branch: "public".into(),
            language: "english".into(),
            owned: HashSet::new(),
            disabled_dlc: vec![],
            verify: false,
        }
    }

    fn ids(p: &[PlannedDepot]) -> Vec<u32> {
        p.iter().map(|d| d.depot_id).collect()
    }

    #[test]
    fn selects_depots_per_runtime() {
        assert_eq!(ids(&plan(&req(Runtime::Native)).unwrap()), vec![13, 14]);
        assert_eq!(ids(&plan(&req(Runtime::Proton)).unwrap()), vec![11, 14]);
    }

    #[test]
    fn language_dlc_and_branch() {
        let mut r = req(Runtime::Native);
        r.language = "german".into();
        assert_eq!(ids(&plan(&r).unwrap()), vec![13, 14, 15]);

        let mut r = req(Runtime::Native);
        r.owned.insert(900);
        assert_eq!(ids(&plan(&r).unwrap()), vec![13, 14, 16]);
        r.disabled_dlc.push(900);
        assert_eq!(ids(&plan(&r).unwrap()), vec![13, 14]);

        let mut r = req(Runtime::Native);
        r.branch = "beta".into();
        let p = plan(&r).unwrap();
        assert_eq!(p[0].manifest.gid, 33);
        assert_eq!(
            p[1].manifest.gid, 4,
            "falls back to public for depots without a beta manifest"
        );
    }

    #[test]
    fn update_detection() {
        let a = app();
        let mut m = AppManifest {
            app_id: 10,
            runtime: Some("native".into()),
            ..Default::default()
        };
        m.installed_depots.insert(
            13,
            InstalledDepot {
                manifest: 3,
                size: 100,
                dlc_app_id: None,
            },
        );
        m.installed_depots.insert(
            14,
            InstalledDepot {
                manifest: 4,
                size: 5,
                dlc_app_id: None,
            },
        );
        assert!(!needs_update(&a, &m));
        m.installed_depots.get_mut(&13).unwrap().manifest = 2;
        assert!(needs_update(&a, &m));
        m.runtime = None; // official-client manifest: inferred from the linux depot
        assert!(needs_update(&a, &m));
        let _ = Branch::default();
    }
}
