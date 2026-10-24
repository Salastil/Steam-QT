//! Steam Workshop: keep subscribed items downloaded for installed games.
//!
//! Items live where Steam puts them, `steamapps/workshop/content/<appid>/<itemid>/`, and are
//! tracked in `steamapps/workshop/appworkshop_<appid>.acf` in Steam's format, so games (which
//! read the folder through ISteamUGC) and the official client both see them. Item content is a
//! depot manifest (`hcontent_file`) on the app's workshop depot, downloaded like game content;
//! legacy items are a single file at `file_url`. The flow follows steamroom-cli's `workshop`
//! command and SteamKit2's UGC handling.

use crate::install::{self, CdnContext, InstallError, PlannedDepot};
use crate::library::LibraryFolder;
use crate::pics::{AppDetails, DepotManifestRef};
use crate::session::{Session, SessionError, http_client};
use crate::vdf::{self, Node};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use steam_vent::ConnectionTrait;
use steam_vent_proto::steammessages_publishedfile_steamclient::{
    CPublishedFile_GetDetails_Request, CPublishedFile_GetUserFiles_Request, PublishedFileDetails,
};
use steamroom::cdn::CdnServerPool;
use steamroom::depot::DepotId;
use steamroom_client::download::{CdnChunkFetcher, DepotJob};
use tracing::{info, warn};

#[derive(Clone, Debug, PartialEq)]
pub struct WorkshopItem {
    pub id: u64,
    pub title: String,
    pub app_id: u32,
    /// Content manifest on the app's workshop depot (0 for legacy single-file items).
    pub manifest: u64,
    pub file_url: Option<String>,
    pub filename: Option<String>,
    pub size: u64,
    pub time_updated: u32,
}

impl From<&PublishedFileDetails> for WorkshopItem {
    fn from(d: &PublishedFileDetails) -> Self {
        Self {
            id: d.publishedfileid.unwrap_or(0),
            title: d.title.clone().unwrap_or_default(),
            app_id: d.consumer_appid.unwrap_or(0),
            manifest: d.hcontent_file.unwrap_or(0),
            file_url: d.file_url.clone().filter(|u| !u.is_empty()),
            filename: d.filename.clone().filter(|f| !f.is_empty()),
            size: d.file_size.unwrap_or(0),
            time_updated: d.time_updated.unwrap_or(0),
        }
    }
}

impl Session {
    /// Items the logged-in user subscribes to for `app_id`.
    pub async fn subscribed_items(&self, app_id: u32) -> Result<Vec<WorkshopItem>, SessionError> {
        let mut out = Vec::new();
        for page in 1.. {
            let req = CPublishedFile_GetUserFiles_Request {
                steamid: Some(self.steam_id()),
                appid: Some(app_id),
                type_: Some("mysubscriptions".into()),
                numperpage: Some(100),
                page: Some(page),
                ..Default::default()
            };
            let resp = self.connection().service_method(req).await?;
            let batch: Vec<WorkshopItem> = resp
                .publishedfiledetails
                .iter()
                .filter(|d| d.result.unwrap_or(1) == 1 && !d.banned.unwrap_or(false))
                .map(WorkshopItem::from)
                .collect();
            let total = resp.total.unwrap_or(0) as usize;
            let got = resp.publishedfiledetails.len();
            out.extend(batch);
            if got == 0 || page as usize * 100 >= total {
                break;
            }
        }
        Ok(out)
    }

    pub async fn workshop_details(&self, ids: &[u64]) -> Result<Vec<WorkshopItem>, SessionError> {
        let req = CPublishedFile_GetDetails_Request {
            publishedfileids: ids.to_vec(),
            includechildren: Some(true),
            ..Default::default()
        };
        let resp = self.connection().service_method(req).await?;
        Ok(resp
            .publishedfiledetails
            .iter()
            .map(WorkshopItem::from)
            .collect())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstalledItem {
    pub size: u64,
    pub time_updated: u32,
    pub manifest: u64,
}

/// `appworkshop_<appid>.acf`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkshopState {
    pub app_id: u32,
    pub items: BTreeMap<u64, InstalledItem>,
    pub time_last_updated: u64,
}

pub fn state_path(lib: &LibraryFolder, app_id: u32) -> PathBuf {
    lib.steamapps()
        .join("workshop")
        .join(format!("appworkshop_{app_id}.acf"))
}

impl WorkshopState {
    pub fn load(lib: &LibraryFolder, app_id: u32) -> Self {
        let Some(kv) = std::fs::read_to_string(state_path(lib, app_id))
            .ok()
            .and_then(|t| vdf::parse(&t))
        else {
            return Self {
                app_id,
                ..Default::default()
            };
        };
        let items = vdf::children(kv.get("WorkshopItemsInstalled"))
            .into_iter()
            .filter_map(|(id, node)| {
                Some((
                    id.parse().ok()?,
                    InstalledItem {
                        size: vdf::get_u64(node, "size").unwrap_or(0),
                        time_updated: vdf::get_u64(node, "timeupdated").unwrap_or(0) as u32,
                        manifest: vdf::get_u64(node, "manifest").unwrap_or(0),
                    },
                ))
            })
            .collect();
        Self {
            app_id,
            items,
            time_last_updated: vdf::get_u64(&kv, "TimeLastUpdated").unwrap_or(0),
        }
    }

    pub fn to_vdf(&self) -> String {
        let mut root = Node::table();
        let size: u64 = self.items.values().map(|i| i.size).sum();
        root.val("appid", self.app_id)
            .val("SizeOnDisk", size)
            .val("NeedsUpdate", 0)
            .val("NeedsDownload", 0)
            .val("TimeLastUpdated", self.time_last_updated)
            .val("TimeLastAppRan", 0);
        let mut installed = Node::table();
        let mut details = Node::table();
        for (id, item) in &self.items {
            let mut n = Node::table();
            n.val("size", item.size)
                .val("timeupdated", item.time_updated)
                .val("manifest", item.manifest);
            installed.set(id.to_string(), n);
            let mut d = Node::table();
            d.val("manifest", item.manifest)
                .val("timeupdated", item.time_updated)
                .val("timetouched", item.time_updated);
            details.set(id.to_string(), d);
        }
        root.set("WorkshopItemsInstalled", installed);
        root.set("WorkshopItemDetails", details);
        vdf::to_string("AppWorkshop", &root)
    }

    pub fn save(&self, lib: &LibraryFolder) -> std::io::Result<()> {
        let path = state_path(lib, self.app_id);
        std::fs::create_dir_all(path.parent().expect("has parent"))?;
        let tmp = path.with_extension("acf.tmp");
        std::fs::write(&tmp, self.to_vdf())?;
        std::fs::rename(tmp, path)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncReport {
    pub downloaded: usize,
    pub removed: usize,
    pub up_to_date: usize,
    pub failed: Vec<(u64, String)>,
}

/// Bring the workshop folder in line with the user's subscriptions for `app`.
pub async fn sync(
    session: &Session,
    app: &AppDetails,
    lib: &LibraryFolder,
) -> Result<SyncReport, InstallError> {
    let items = session.subscribed_items(app.app_id).await?;
    sync_items(session, app, lib, &items).await
}

pub async fn sync_items(
    session: &Session,
    app: &AppDetails,
    lib: &LibraryFolder,
    items: &[WorkshopItem],
) -> Result<SyncReport, InstallError> {
    let mut state = WorkshopState::load(lib, app.app_id);
    let mut report = SyncReport::default();
    let content = lib.workshop_content(app.app_id);

    // Unsubscribed items: remove.
    let wanted: std::collections::HashSet<u64> = items.iter().map(|i| i.id).collect();
    let stale: Vec<u64> = state
        .items
        .keys()
        .filter(|id| !wanted.contains(id))
        .copied()
        .collect();
    for id in stale {
        let _ = std::fs::remove_dir_all(content.join(id.to_string()));
        state.items.remove(&id);
        report.removed += 1;
    }

    let depot_id = app.workshop_depot.unwrap_or(app.app_id);
    let mut cdn: Option<CdnContext> = None;
    for item in items {
        let installed = state.items.get(&item.id);
        let dir = content.join(item.id.to_string());
        let current = installed
            .is_some_and(|i| i.manifest == item.manifest && i.time_updated == item.time_updated)
            && dir.exists();
        if current {
            report.up_to_date += 1;
            continue;
        }
        let result = if item.manifest != 0 {
            if cdn.is_none() {
                cdn = Some(CdnContext::new(session).await?);
            }
            let old = installed
                .map(|i| i.manifest)
                .filter(|&m| m != 0 && m != item.manifest);
            download_item(
                session,
                cdn.as_ref().expect("set above"),
                app.app_id,
                depot_id,
                item,
                old,
                lib,
                &dir,
            )
            .await
        } else if let Some(url) = &item.file_url {
            download_legacy(url, item.filename.as_deref(), &dir).await
        } else {
            Err(InstallError::Depot(depot_id, "item has no content".into()))
        };
        match result {
            Ok(()) => {
                state.items.insert(
                    item.id,
                    InstalledItem {
                        size: item.size,
                        time_updated: item.time_updated,
                        manifest: item.manifest,
                    },
                );
                report.downloaded += 1;
                info!(item = item.id, title = %item.title, "workshop item updated");
            }
            Err(e) => {
                warn!(item = item.id, error = %e, "workshop item failed");
                report.failed.push((item.id, e.to_string()));
            }
        }
    }
    state.time_last_updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    state.save(lib)?;
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
async fn download_item(
    session: &Session,
    cdn: &CdnContext,
    app_id: u32,
    depot_id: u32,
    item: &WorkshopItem,
    old_manifest: Option<u64>,
    lib: &LibraryFolder,
    dir: &std::path::Path,
) -> Result<(), InstallError> {
    let key = session.depot_key(app_id, depot_id).await?;
    let planned = PlannedDepot {
        depot_id,
        manifest: DepotManifestRef {
            gid: item.manifest,
            size: item.size,
            download: item.size,
        },
        dlc_app_id: None,
    };
    let manifest = install::fetch_manifest(session, cdn, app_id, &planned, None, &key, lib).await?;
    let mut builder = DepotJob::builder()
        .depot_id(DepotId(depot_id))
        .depot_key(key.clone())
        .install_dir(dir.to_path_buf());
    if let Some(old) =
        old_manifest.and_then(|m| install::load_cached_manifest(lib, depot_id, m, &key))
    {
        builder = builder
            .old_manifest_files(old.files.iter().map(|f| f.normalized_path()).collect())
            .old_file_layouts(install::old_file_layouts(&old));
    }
    let job = builder
        .build()
        .map_err(|e| InstallError::Depot(depot_id, e.to_string()))?;
    let token = session
        .cdn_auth_token(app_id, depot_id, &cdn.servers[0].host)
        .await
        .ok()
        .flatten();
    let fetcher = Arc::new(CdnChunkFetcher::new(
        steamroom::cdn::CdnClient::new().map_err(|e| InstallError::Cdn(e.to_string()))?,
        CdnServerPool::new(cdn.servers.clone()),
        token,
    ));
    job.download(&manifest, fetcher)
        .await
        .map_err(|e| InstallError::Depot(depot_id, e.to_string()))?;
    let _ = std::fs::remove_dir_all(dir.join(".DepotDownloader"));
    if let Some(old) = old_manifest {
        let _ = std::fs::remove_file(install::cached_manifest_path(lib, depot_id, old));
    }
    Ok(())
}

async fn download_legacy(
    url: &str,
    filename: Option<&str>,
    dir: &std::path::Path,
) -> Result<(), InstallError> {
    let name = filename
        .and_then(|f| f.rsplit(['/', '\\']).next())
        .filter(|f| !f.is_empty())
        .unwrap_or("item.bin");
    let bytes = http_client()
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| InstallError::Cdn(e.to_string()))?
        .bytes()
        .await
        .map_err(|e| InstallError::Cdn(e.to_string()))?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(name), &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trip() {
        let dir = std::env::temp_dir().join(format!("steamqt-ws-{}", std::process::id()));
        let lib = LibraryFolder::new(&dir);
        let mut s = WorkshopState {
            app_id: 4000,
            time_last_updated: 1700000000,
            ..Default::default()
        };
        s.items.insert(
            123,
            InstalledItem {
                size: 10,
                time_updated: 5,
                manifest: 99,
            },
        );
        s.items.insert(
            456,
            InstalledItem {
                size: 20,
                time_updated: 6,
                manifest: 0,
            },
        );
        s.save(&lib).unwrap();
        assert_eq!(WorkshopState::load(&lib, 4000), s);
        let text = std::fs::read_to_string(state_path(&lib, 4000)).unwrap();
        assert!(text.starts_with("\"AppWorkshop\""));
        assert!(text.contains("\"SizeOnDisk\"\t\t\"30\""));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
