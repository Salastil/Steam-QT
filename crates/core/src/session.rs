//! Logged-in Steam session.
//!
//! Login is split in two phases, each using the open-source library best suited to it:
//!
//! 1. **Authentication** (password → Steam Guard → refresh token) uses steamroom-client's
//!    typestate flow, which exposes each Steam Guard step so the UI can prompt for it and
//!    hands back the refresh token for "remember me".
//! 2. **The session** (licences, PICS, depot keys, CDN tokens, heartbeats) uses steam-vent,
//!    whose message filter routes responses by job id and keeps unsolicited messages, so
//!    concurrent requests from the UI and the download queue don't interfere.
//!
//! steam-vent's own password login performs the same `ClientLogon` with the refresh token in
//! the `access_token` field that [`Session::resume`] does here.

use crate::net;
use crate::pics::{self, AppDetails, PackageDetails};
use futures_util::{StreamExt, TryStreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use steam_vent::connection::UnAuthenticatedConnection;
use steam_vent::{Connection, ConnectionTrait};
use steam_vent_proto::steammessages_clientserver::{
    CMsgClientLicenseList, cmsg_client_license_list,
};
use steam_vent_proto::steammessages_clientserver_2::{
    CMsgClientGetDepotDecryptionKey, CMsgClientGetDepotDecryptionKeyResponse,
};
use steam_vent_proto::steammessages_clientserver_appinfo::{
    CMsgClientPICSAccessTokenRequest, CMsgClientPICSAccessTokenResponse,
    CMsgClientPICSChangesSinceRequest, CMsgClientPICSChangesSinceResponse,
    CMsgClientPICSProductInfoRequest, CMsgClientPICSProductInfoResponse,
    cmsg_client_picsproduct_info_request,
};
use steam_vent_proto::steammessages_contentsystem_steamclient::{
    CContentServerDirectory_GetCDNAuthToken_Request,
    CContentServerDirectory_GetManifestRequestCode_Request,
    CContentServerDirectory_GetServersForSteamPipe_Request,
};
use steamroom::cdn::server::CdnServer;
use steamroom::client::SteamClient;
use steamroom::depot::DepotKey;
use steamroom_client::login::{
    ApprovedAuth, ClientOs, CredentialsLoginFlow, GuardChallenge, GuardType, LoginError,
    MobileChallenge, PreparedLoginBuilder,
};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// `EOSType` reported at logon. -203 is `k_eLinuxUnknown`, which the official Linux client
/// falls back to when it can't classify the kernel.
const CLIENT_OS: ClientOs = ClientOs::new(-203);
const PICS_BATCH: usize = 100;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("network: {0}")]
    Net(#[from] net::NetError),
    #[error("could not reach Steam's server directory: {0}")]
    Directory(String),
    #[error("{0}")]
    Connection(#[from] steam_vent::ConnectionError),
    #[error("{0}")]
    Network(#[from] steam_vent::NetworkError),
    #[error("{0}")]
    Login(#[from] LoginError),
    #[error("Steam returned error {0} for {1}")]
    EResult(i32, &'static str),
    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = SessionError> = std::result::Result<T, E>;

/// Pick connection-manager WebSocket endpoints from Steam's directory, best first.
pub async fn discover_cms(cell_id: u32) -> Result<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct Entry {
        endpoint: String,
        #[serde(rename = "type")]
        kind: String,
    }
    #[derive(serde::Deserialize)]
    struct Inner {
        serverlist: Vec<Entry>,
    }
    #[derive(serde::Deserialize)]
    struct Outer {
        response: Inner,
    }
    let url = format!(
        "https://api.steampowered.com/ISteamDirectory/GetCMListForConnect/v1/?cellid={cell_id}"
    );
    let resp: Outer = http_client()
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| SessionError::Directory(e.to_string()))?
        .json()
        .await
        .map_err(|e| SessionError::Directory(e.to_string()))?;
    let list: Vec<String> = resp
        .response
        .serverlist
        .into_iter()
        .filter(|e| e.kind == "websockets")
        .map(|e| e.endpoint)
        .collect();
    // Port 443 passes restrictive firewalls and HTTPS proxies; keep load order otherwise.
    let mut list = list;
    list.sort_by_key(|e| !e.ends_with(":443"));
    if list.is_empty() {
        return Err(SessionError::Directory(
            "no WebSocket servers listed".into(),
        ));
    }
    Ok(list)
}

pub fn http_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(concat!("Steam-QT/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(15))
                .build()
                .expect("reqwest client")
        })
        .clone()
}

async fn connect_any<T, F, Fut>(f: F) -> Result<T>
where
    F: Fn(net::WsStream) -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last = SessionError::Directory("no servers tried".into());
    for endpoint in discover_cms(0).await?.iter().take(6) {
        let url = net::cm_url(endpoint);
        match net::connect_ws(&url).await {
            Ok(ws) => match f(ws).await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    warn!(%endpoint, error = %e, "CM handshake failed");
                    last = e;
                }
            },
            Err(e) => {
                warn!(%endpoint, error = %e, "CM connect failed");
                last = e.into();
            }
        }
    }
    Err(last)
}

fn device_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|h| format!("Steam-QT on {h}"))
        .unwrap_or_else(|| "Steam-QT".into())
}

/// Tokens from a completed authentication. The refresh token is what "remember me" stores.
#[derive(Clone, Debug)]
pub struct Credentials {
    pub account: String,
    pub refresh_token: String,
}

/// Interactive authentication state, driven by the UI.
pub enum AuthStep {
    Done(Credentials),
    /// Steam wants a code; `email` is true for an emailed code, false for the authenticator app.
    NeedCode {
        email: bool,
        challenge: CodeChallenge,
    },
    /// Steam is waiting for approval in the mobile app.
    NeedMobileApproval(MobileApproval),
}

pub struct CodeChallenge {
    account: String,
    inner: GuardChallenge,
    kind: GuardType,
}

pub struct MobileApproval {
    account: String,
    inner: MobileChallenge,
}

fn finish(account: &str, approved: ApprovedAuth) -> Credentials {
    let tokens = approved.tokens();
    Credentials {
        account: tokens
            .account_name
            .clone()
            .unwrap_or_else(|| account.to_string()),
        refresh_token: tokens.refresh_token.clone(),
    }
}

impl CodeChallenge {
    /// Submit a code. On a wrong code the challenge comes back for another attempt.
    pub async fn submit(
        self,
        code: &str,
    ) -> std::result::Result<Credentials, (Option<Self>, SessionError)> {
        let Self {
            account,
            inner,
            kind,
        } = self;
        match inner.submit_code(code.trim(), kind).await {
            Ok(approved) => Ok(finish(&account, approved)),
            Err((inner, LoginError::InvalidGuardCode)) => Err((
                Some(Self {
                    account,
                    inner,
                    kind,
                }),
                LoginError::InvalidGuardCode.into(),
            )),
            Err((_, e)) => Err((None, e.into())),
        }
    }
}

impl MobileApproval {
    pub async fn wait(self) -> Result<Credentials> {
        let approved = self.inner.wait_for_confirmation().await?;
        Ok(finish(&self.account, approved))
    }
}

/// Start a password login.
pub async fn authenticate(account: &str, password: &str) -> Result<AuthStep> {
    let account = account.trim().to_string();
    let password = password.to_string();
    let flow = connect_any(|ws| {
        let (account, password) = (account.clone(), password.clone());
        async move {
            let (client, _rx) = SteamClient::connect_ws(net::RoomTransport::new(ws))
                .await
                .map_err(LoginError::from)?;
            let ready = client.prepare().await.map_err(LoginError::from)?;
            Ok(PreparedLoginBuilder::new(ready)
                .device_name(device_name())
                .client_os(CLIENT_OS)
                .with_credentials(account, password)
                .begin()
                .await?)
        }
    })
    .await?;
    Ok(match flow {
        CredentialsLoginFlow::Approved(approved) => AuthStep::Done(finish(&account, approved)),
        CredentialsLoginFlow::NeedsGuardCode(inner) => {
            let kind = if inner.allowed_kinds().contains(&GuardType::DeviceCode) {
                GuardType::DeviceCode
            } else {
                GuardType::EmailCode
            };
            AuthStep::NeedCode {
                email: kind == GuardType::EmailCode,
                challenge: CodeChallenge {
                    account,
                    inner,
                    kind,
                },
            }
        }
        CredentialsLoginFlow::NeedsMobileConfirm(inner) => {
            AuthStep::NeedMobileApproval(MobileApproval { account, inner })
        }
        _ => return Err(SessionError::Other("unsupported Steam Guard flow".into())),
    })
}

#[derive(Clone, Debug)]
pub struct License {
    pub package_id: u32,
    pub access_token: u64,
    pub license_type: u32,
    pub flags: u32,
    pub time_created: u32,
}

impl From<&cmsg_client_license_list::License> for License {
    fn from(l: &cmsg_client_license_list::License) -> Self {
        Self {
            package_id: l.package_id.unwrap_or(0),
            access_token: l.access_token.unwrap_or(0),
            license_type: l.license_type.unwrap_or(0),
            flags: l.flags.unwrap_or(0),
            time_created: l.time_created.unwrap_or(0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PicsChanges {
    pub current: u32,
    pub apps: Option<Vec<u32>>,
    pub packages_changed: bool,
}

#[derive(Clone, Debug)]
pub struct RawAppInfo {
    pub app_id: u32,
    pub change_number: u32,
    pub buffer: Option<Vec<u8>>,
}

/// A logged-in (or anonymous) connection to Steam.
#[derive(Clone)]
pub struct Session {
    conn: Connection,
    account: Option<String>,
    licenses: Arc<RwLock<Vec<License>>>,
}

impl Session {
    /// Log in with a stored refresh token.
    pub async fn resume(credentials: &Credentials) -> Result<Self> {
        let account = credentials.account.clone();
        let token = credentials.refresh_token.clone();
        let conn = connect_any(|ws| {
            let (account, token) = (account.clone(), token.clone());
            async move {
                let (tx, rx) = net::vent_pair(ws);
                let unauth = UnAuthenticatedConnection::from_sender_receiver(tx, rx).await?;
                Ok(unauth.access(&account, &token).await?)
            }
        })
        .await?;
        info!(steam_id = u64::from(conn.steam_id()), "logged in");
        Self::start(conn, Some(credentials.account.clone())).await
    }

    /// Anonymous session: free content and dedicated servers only.
    pub async fn anonymous() -> Result<Self> {
        let conn = connect_any(|ws| async move {
            let (tx, rx) = net::vent_pair(ws);
            let unauth = UnAuthenticatedConnection::from_sender_receiver(tx, rx).await?;
            Ok(unauth.anonymous().await?)
        })
        .await?;
        Self::start(conn, None).await
    }

    async fn start(conn: Connection, account: Option<String>) -> Result<Self> {
        // Register for the licence list before inspecting buffered messages, so the copy
        // Steam sends right after logon is seen whichever way the race goes.
        let first = conn.one::<CMsgClientLicenseList>();
        let buffered = conn
            .take_unprocessed()
            .into_iter()
            .filter_map(|raw| raw.into_message::<CMsgClientLicenseList>().ok())
            .last();
        let list = match buffered {
            Some(list) => Some(list),
            // Anonymous sessions get no licence list, so don't wait long for one.
            None => {
                let wait = Duration::from_secs(if account.is_some() { 15 } else { 2 });
                tokio::time::timeout(wait, first)
                    .await
                    .ok()
                    .and_then(|r| r.ok())
            }
        };
        let licenses: Vec<License> = list
            .map(|l| l.licenses.iter().map(License::from).collect())
            .unwrap_or_default();
        debug!(count = licenses.len(), "licences received");

        let session = Self {
            conn,
            account,
            licenses: Arc::new(RwLock::new(licenses)),
        };
        session.watch_licenses();
        Ok(session)
    }

    /// Keep the licence list current when Steam pushes changes (purchases, free weekends).
    fn watch_licenses(&self) {
        let licenses = Arc::downgrade(&self.licenses);
        let mut updates = Box::pin(self.conn.on::<CMsgClientLicenseList>());
        tokio::spawn(async move {
            while let Some(Ok(list)) = updates.next().await {
                let Some(target) = licenses.upgrade() else {
                    break;
                };
                *target.write().await = list.licenses.iter().map(License::from).collect();
            }
        });
    }

    pub fn account(&self) -> Option<&str> {
        self.account.as_deref()
    }

    pub fn steam_id(&self) -> u64 {
        u64::from(self.conn.steam_id())
    }

    pub fn cell_id(&self) -> u32 {
        self.conn.cell_id()
    }

    pub async fn licenses(&self) -> Vec<License> {
        self.licenses.read().await.clone()
    }

    /// PICS access tokens for apps (needed for apps whose info isn't public).
    pub async fn app_access_tokens(&self, app_ids: &[u32]) -> Result<HashMap<u32, u64>> {
        let req = CMsgClientPICSAccessTokenRequest {
            appids: app_ids.to_vec(),
            ..Default::default()
        };
        let resp: CMsgClientPICSAccessTokenResponse = self.conn.job(req).await?;
        Ok(resp
            .app_access_tokens
            .iter()
            .filter_map(|t| Some((t.appid?, t.access_token?)))
            .collect())
    }

    async fn product_info(
        &self,
        req: CMsgClientPICSProductInfoRequest,
    ) -> Result<Vec<CMsgClientPICSProductInfoResponse>> {
        Ok(self.conn.job_multi(req).try_collect().await?)
    }

    /// Package details for the given packages (with their licence access tokens).
    pub async fn packages(&self, packages: &[(u32, u64)]) -> Result<Vec<PackageDetails>> {
        let mut out = Vec::with_capacity(packages.len());
        for batch in packages.chunks(PICS_BATCH) {
            let req = CMsgClientPICSProductInfoRequest {
                packages: batch
                    .iter()
                    .map(
                        |&(id, token)| cmsg_client_picsproduct_info_request::PackageInfo {
                            packageid: Some(id),
                            access_token: Some(token),
                            ..Default::default()
                        },
                    )
                    .collect(),
                meta_data_only: Some(false),
                ..Default::default()
            };
            for resp in self.product_info(req).await? {
                for p in &resp.packages {
                    let (Some(id), Some(buf)) = (p.packageid, p.buffer.as_deref()) else {
                        continue;
                    };
                    if let Some(kv) = pics::parse_package_buffer(buf) {
                        out.push(PackageDetails::from_kv(id, &kv));
                    }
                }
            }
        }
        Ok(out)
    }

    /// App details, fetching access tokens as needed.
    pub async fn apps(&self, app_ids: &[u32]) -> Result<Vec<AppDetails>> {
        Ok(self
            .app_buffers(app_ids, false)
            .await?
            .into_iter()
            .filter_map(|r| {
                let kv = pics::parse_app_buffer(r.buffer.as_deref()?)?;
                Some(AppDetails::from_kv(r.app_id, r.change_number, &kv))
            })
            .collect())
    }

    /// Raw PICS app info. With `meta_only`, only change numbers are returned (cheap), which
    /// lets callers skip apps whose cached info is current.
    pub async fn app_buffers(&self, app_ids: &[u32], meta_only: bool) -> Result<Vec<RawAppInfo>> {
        let mut out = Vec::with_capacity(app_ids.len());
        for batch in app_ids.chunks(PICS_BATCH) {
            let tokens = self.app_access_tokens(batch).await.unwrap_or_default();
            let req = CMsgClientPICSProductInfoRequest {
                apps: batch
                    .iter()
                    .map(|id| cmsg_client_picsproduct_info_request::AppInfo {
                        appid: Some(*id),
                        access_token: Some(tokens.get(id).copied().unwrap_or(0)),
                        ..Default::default()
                    })
                    .collect(),
                meta_data_only: Some(meta_only),
                ..Default::default()
            };
            for resp in self.product_info(req).await? {
                for a in &resp.apps {
                    let Some(id) = a.appid else { continue };
                    let buffer = if meta_only {
                        None
                    } else {
                        match a.buffer.as_deref() {
                            Some(b) => Some(b.to_vec()),
                            None => match (resp.http_host.as_deref(), a.sha.as_deref()) {
                                (Some(host), Some(sha)) => fetch_appinfo_http(host, id, sha).await,
                                _ => None,
                            },
                        }
                    };
                    out.push(RawAppInfo {
                        app_id: id,
                        change_number: a.change_number.unwrap_or(0),
                        buffer,
                    });
                }
            }
        }
        Ok(out)
    }

    /// Apps whose PICS info changed since `since` (0 just returns the current change number).
    /// `None` in the app list means Steam asked for a full refresh.
    pub async fn changes_since(&self, since: u32) -> Result<PicsChanges> {
        let req = CMsgClientPICSChangesSinceRequest {
            since_change_number: Some(since),
            send_app_info_changes: Some(true),
            send_package_info_changes: Some(true),
            ..Default::default()
        };
        let resp: CMsgClientPICSChangesSinceResponse = self.conn.job(req).await?;
        let full =
            resp.force_full_update.unwrap_or(false) || resp.force_full_app_update.unwrap_or(false);
        Ok(PicsChanges {
            current: resp.current_change_number.unwrap_or(since),
            apps: (!full).then(|| resp.app_changes.iter().filter_map(|c| c.appid).collect()),
            packages_changed: !resp.package_changes.is_empty()
                || resp.force_full_package_update.unwrap_or(false),
        })
    }

    pub async fn depot_key(&self, app_id: u32, depot_id: u32) -> Result<DepotKey> {
        let req = CMsgClientGetDepotDecryptionKey {
            depot_id: Some(depot_id),
            app_id: Some(app_id),
            ..Default::default()
        };
        let resp: CMsgClientGetDepotDecryptionKeyResponse = self.conn.job(req).await?;
        let eresult = resp.eresult.unwrap_or(2);
        let key = resp.depot_encryption_key.unwrap_or_default();
        match <[u8; 32]>::try_from(key.as_slice()) {
            Ok(k) if eresult == 1 => Ok(DepotKey(k)),
            _ => Err(SessionError::EResult(eresult, "depot key")),
        }
    }

    pub async fn cdn_servers(&self) -> Result<Vec<CdnServer>> {
        let req = CContentServerDirectory_GetServersForSteamPipe_Request {
            cell_id: Some(self.cell_id()),
            max_servers: Some(20),
            ..Default::default()
        };
        let resp = self.conn.service_method(req).await?;
        Ok(resp
            .servers
            .iter()
            .filter(|s| matches!(s.type_.as_deref(), Some("CDN" | "SteamCache")))
            .filter_map(|s| {
                let host = s.host.clone()?;
                let https = matches!(s.https_support.as_deref(), Some("mandatory" | "optional"));
                let (host, port) = match host.rsplit_once(':') {
                    Some((h, p)) => (
                        h.to_string(),
                        p.parse().unwrap_or(if https { 443 } else { 80 }),
                    ),
                    None => (host, if https { 443 } else { 80 }),
                };
                let mut server = steamroom_cdn_server(host.clone(), port, https);
                server.vhost = s.vhost.clone().unwrap_or(host);
                Some(server)
            })
            .collect())
    }

    /// Request code required to download a manifest from the CDN (0 if not needed).
    pub async fn manifest_request_code(
        &self,
        app_id: u32,
        depot_id: u32,
        manifest_id: u64,
        branch: &str,
    ) -> Result<u64> {
        let req = CContentServerDirectory_GetManifestRequestCode_Request {
            app_id: Some(app_id),
            depot_id: Some(depot_id),
            manifest_id: Some(manifest_id),
            app_branch: Some(branch.to_string()),
            ..Default::default()
        };
        Ok(self
            .conn
            .service_method(req)
            .await?
            .manifest_request_code
            .unwrap_or(0))
    }

    pub async fn cdn_auth_token(
        &self,
        app_id: u32,
        depot_id: u32,
        host: &str,
    ) -> Result<Option<String>> {
        let req = CContentServerDirectory_GetCDNAuthToken_Request {
            app_id: Some(app_id),
            depot_id: Some(depot_id),
            host_name: Some(host.to_string()),
            ..Default::default()
        };
        Ok(self
            .conn
            .service_method(req)
            .await?
            .token
            .filter(|t| !t.is_empty()))
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

/// `CdnServer` is `#[non_exhaustive]`, so build it through serde.
fn steamroom_cdn_server(host: String, port: u16, https: bool) -> CdnServer {
    let template = serde_json::json!({ "host": host, "port": port, "https": https, "vhost": "" });
    serde_json::from_value(template).expect("CdnServer fields")
}

/// Large app info blobs are served over HTTP instead of inline (SteamKit2 `PICSProductInfo`).
async fn fetch_appinfo_http(host: &str, app_id: u32, sha: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let hex: String = sha.iter().map(|b| format!("{b:02x}")).collect();
    let url = format!("https://{host}/appinfo/{app_id}/sha/{hex}.txt.gz");
    let body = http_client()
        .get(&url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .bytes()
        .await
        .ok()?;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(&body[..])
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}
