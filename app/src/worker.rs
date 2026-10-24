//! Backend worker: owns the Steam session, library and download queue on a tokio runtime.
//!
//! The Qt side sends [`Command`]s and receives [`UiEvent`]s; nothing here touches Qt, so the
//! whole flow can be exercised without a GUI.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use steamqt_core::catalog::{self, Catalog, Entry};
use steamqt_core::config::{self, Settings};
use steamqt_core::install::{self, InstallRequest, Progress, Runtime};
use steamqt_core::launch::{self, CompatTool};
use steamqt_core::library::{self, LibraryFolder};
use steamqt_core::pics::AppDetails;
use steamqt_core::session::{self, AuthStep, CodeChallenge, Credentials, Session};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthState {
    LoggedOut,
    Working,
    NeedCode,
    NeedEmailCode,
    NeedMobile,
    Ready,
}

impl AuthState {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthState::LoggedOut => "loggedOut",
            AuthState::Working => "working",
            AuthState::NeedCode => "needCode",
            AuthState::NeedEmailCode => "needEmailCode",
            AuthState::NeedMobile => "needMobile",
            AuthState::Ready => "ready",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Native,
    Proton,
}

#[derive(Debug)]
pub enum Command {
    Login {
        account: String,
        password: String,
        remember: bool,
    },
    SubmitCode(String),
    Logout,
    Refresh,
    SetFilter {
        text: String,
        mode: Filter,
    },
    ToggleSection(String),
    Install {
        app_id: u32,
        library: Option<usize>,
        runtime: Option<Runtime>,
    },
    Update(u32),
    Verify(u32),
    Pause(u32),
    Uninstall(u32),
    SetRuntime {
        app_id: u32,
        runtime: Runtime,
    },
    SetDlc {
        app_id: u32,
        dlc: u32,
        enabled: bool,
    },
    AddLibrary(PathBuf),
    RemoveLibrary(usize),
    SetDefaultLibrary(usize),
    SetAutoUpdate(bool),
    Play(u32),
    Stop(u32),
    SetDefaultProton(String),
    /// Empty key = use the default.
    SetProtonFor {
        app_id: u32,
        key: String,
    },
    SetLaunchOptions {
        app_id: u32,
        options: String,
    },
}

/// One row of the library list: either a section header or a game.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Row {
    pub header: bool,
    pub section: String,
    pub title: String,
    pub extra: String,
    pub expanded: bool,
    pub app_id: u32,
    /// "both", "linux" or "windows".
    pub os: String,
    /// "native" or "proton".
    pub runtime: String,
    /// "ready", "update", "downloading", "queued", "paused" or "none".
    pub state: String,
    pub status: String,
    pub size: String,
    pub progress: f64,
    pub icon_url: String,
    pub build: String,
    pub branch: String,
    pub updated: String,
    pub location: String,
    /// JSON list of {id, name, enabled, content}.
    pub dlc_json: String,
    /// Compatibility tool used for this game (Proton builds only).
    pub proton: String,
    /// Selected tool key, empty when following the default.
    pub proton_key: String,
    pub launch_options: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ToolInfo {
    pub key: String,
    pub name: String,
    pub installed: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct LibraryInfo {
    pub path: String,
    pub default: bool,
    pub games: usize,
    pub free: u64,
    pub total: u64,
    pub external: bool,
}

#[derive(Debug)]
pub enum UiEvent {
    Auth {
        state: AuthState,
        error: String,
    },
    Account(String),
    Rows(Vec<Row>),
    Libraries(Vec<LibraryInfo>),
    Footer(String),
    Message(String),
    AutoUpdate(bool),
    Tools {
        tools: Vec<ToolInfo>,
        default: String,
    },
}

pub type Notify = Arc<dyn Fn(UiEvent) + Send + Sync>;

enum Msg {
    Cmd(Command),
    Authenticated(Result<AuthStep, String>),
    CodeResult(Result<Credentials, (Option<CodeChallenge>, String)>),
    LoggedIn(Result<Session, String>, Option<Credentials>),
    CatalogLoaded(Result<Catalog, String>),
    Progress(u32, Progress),
    InstallDone(u32, Result<(), String>),
    ToolsFetched(Result<Vec<AppDetails>, String>),
    Exited(u32, String),
    PollChanges,
    Changes(Result<session::PicsChanges, String>),
    AppsRefreshed(Vec<AppDetails>),
}

pub struct Handle {
    tx: mpsc::UnboundedSender<Command>,
}

impl Handle {
    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }
}

pub fn spawn(notify: Notify) -> Handle {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::Builder::new()
        .name("steamqt-worker".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(Worker::new(notify).run(rx));
        })
        .expect("worker thread");
    Handle { tx }
}

struct ActiveDownload {
    app_id: u32,
    cancel: CancellationToken,
    done: u64,
    total: u64,
    rate: u64,
}

struct Worker {
    notify: Notify,
    tx: mpsc::UnboundedSender<Msg>,
    rx: mpsc::UnboundedReceiver<Msg>,
    settings: Settings,
    session: Option<Session>,
    remember: bool,
    challenge: Option<CodeChallenge>,
    catalog: Catalog,
    filter_text: String,
    filter_mode: Filter,
    collapsed: HashSet<String>,
    queue: VecDeque<(u32, bool)>,
    active: Option<ActiveDownload>,
    paused: HashSet<u32>,
    install_size_cache: HashMap<(u32, Runtime), u64>,
    /// Library chosen in the Install dialog, used when the download starts.
    pending_library: HashMap<u32, usize>,
    /// `STEAMQT_DEMO=440,570,...`: anonymous session with these apps as the library.
    demo: Option<Vec<u32>>,
    /// Tool apps (Proton, Steam Linux Runtime) that aren't library items.
    tools: HashMap<u32, AppDetails>,
    /// Game to start once the downloads it waits for (update, Proton, runtime) finish.
    pending_launch: Option<u32>,
    /// Running games: app id → process group id.
    running: HashMap<u32, i32>,
    /// Last PICS change number seen (0 = not yet known).
    change_number: u32,
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[unit])
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

fn human_eta(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

fn date(ts: u64) -> String {
    if ts == 0 {
        return String::new();
    }
    // Civil-from-days (Howard Hinnant), avoiding a date crate for one label.
    let days = (ts / 86400) as i64;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

impl Worker {
    fn new(notify: Notify) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut settings = Settings::load();
        if settings.libraries.is_empty() {
            // First run: adopt official Steam libraries if present, else propose ~/Games.
            for steam in library::detect_official_steam() {
                for path in steam.libraries {
                    settings.add_library(path);
                }
            }
            if settings.libraries.is_empty() {
                settings.add_library(config::default_library_path());
            }
            let _ = settings.save();
        }
        Self {
            notify,
            tx,
            rx,
            settings,
            session: None,
            remember: true,
            challenge: None,
            catalog: Catalog::default(),
            filter_text: String::new(),
            filter_mode: Filter::All,
            collapsed: ["available".to_string()].into(),
            queue: VecDeque::new(),
            active: None,
            paused: HashSet::new(),
            install_size_cache: HashMap::new(),
            pending_library: HashMap::new(),
            tools: HashMap::new(),
            pending_launch: None,
            running: HashMap::new(),
            change_number: 0,
            demo: std::env::var("STEAMQT_DEMO")
                .ok()
                .map(|v| v.split(',').filter_map(|s| s.trim().parse().ok()).collect()),
        }
    }

    fn emit(&self, event: UiEvent) {
        (self.notify)(event);
    }

    fn auth(&self, state: AuthState, error: impl Into<String>) {
        self.emit(UiEvent::Auth {
            state,
            error: error.into(),
        });
    }

    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        self.publish_libraries();
        self.publish_tools();
        self.emit(UiEvent::AutoUpdate(self.settings.auto_update));
        self.try_resume();
        // Steam's change numbers advance constantly; polling them is cheap and tells us exactly
        // which apps changed, so only those are refetched.
        let mut update_timer = tokio::time::interval(Duration::from_secs(5 * 60));
        update_timer.tick().await;
        loop {
            let msg = tokio::select! {
                Some(cmd) = commands.recv() => Msg::Cmd(cmd),
                Some(msg) = self.rx.recv() => msg,
                _ = update_timer.tick() => Msg::PollChanges,
                else => break,
            };
            self.handle(msg);
        }
    }

    fn open_session(&mut self, creds: Credentials) {
        self.auth(AuthState::Working, "");
        self.emit(UiEvent::Account(creds.account.clone()));
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = Session::resume(&creds).await.map_err(|e| e.to_string());
            let _ = tx.send(Msg::LoggedIn(result, Some(creds)));
        });
    }

    fn try_resume(&mut self) {
        if let Some(apps) = self.demo.clone() {
            // Development mode: anonymous session, the listed apps treated as owned.
            self.auth(AuthState::Working, "");
            self.emit(UiEvent::Account(format!("demo ({} apps)", apps.len())));
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = Session::anonymous().await.map_err(|e| e.to_string());
                let _ = tx.send(Msg::LoggedIn(result, None));
            });
            return;
        }
        let Some(account) = self.settings.remember_account.clone() else {
            self.auth(AuthState::LoggedOut, "");
            return;
        };
        let Some(creds) = config::credentials::load(&account) else {
            self.auth(AuthState::LoggedOut, "");
            return;
        };
        self.auth(AuthState::Working, "");
        self.emit(UiEvent::Account(account));
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = Session::resume(&creds).await.map_err(|e| e.to_string());
            let _ = tx.send(Msg::LoggedIn(result, Some(creds)));
        });
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Cmd(cmd) => self.command(cmd),
            Msg::Authenticated(result) => match result {
                Err(e) => self.auth(AuthState::LoggedOut, e),
                Ok(AuthStep::Done(creds)) => self.open_session(creds),
                Ok(AuthStep::NeedCode { email, challenge }) => {
                    self.challenge = Some(challenge);
                    self.auth(
                        if email {
                            AuthState::NeedEmailCode
                        } else {
                            AuthState::NeedCode
                        },
                        "",
                    );
                }
                Ok(AuthStep::NeedMobileApproval(approval)) => {
                    self.auth(AuthState::NeedMobile, "");
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        let result = approval.wait().await.map_err(|e| (None, e.to_string()));
                        let _ = tx.send(Msg::CodeResult(result));
                    });
                }
            },
            Msg::CodeResult(Ok(creds)) => self.open_session(creds),
            Msg::CodeResult(Err((challenge, e))) => match challenge {
                Some(c) => {
                    self.challenge = Some(c);
                    self.auth(AuthState::NeedCode, format!("{e}. Try again."));
                }
                None => self.auth(AuthState::LoggedOut, e),
            },
            Msg::LoggedIn(Ok(session), creds) => {
                if let Some(creds) = creds.filter(|_| self.remember) {
                    if let Err(e) = config::credentials::save(&creds) {
                        warn!(error = %e, "could not store credentials");
                    }
                    self.settings.remember_account = Some(creds.account.clone());
                    let _ = self.settings.save();
                }
                if let Some(account) = session.account() {
                    self.emit(UiEvent::Account(account.to_string()));
                }
                self.session = Some(session);
                self.auth(AuthState::Ready, "");
                self.load_catalog();
            }
            Msg::LoggedIn(Err(e), creds) => {
                if let Some(c) = creds {
                    // A stored token that no longer works: forget it and ask for the password.
                    if e.contains("AccessDenied")
                        || e.contains("InvalidPassword")
                        || e.contains("Expired")
                    {
                        config::credentials::forget(&c.account);
                    }
                }
                self.auth(AuthState::LoggedOut, e);
            }
            Msg::CatalogLoaded(Ok(catalog)) => {
                if self.catalog.entries.is_empty()
                    && !catalog.entries.iter().any(|e| e.install.is_some())
                {
                    // Nothing installed yet: show what can be installed.
                    self.collapsed.remove("available");
                }
                self.catalog = catalog;
                self.publish();
                self.publish_libraries();
                self.queue_auto_updates();
                if self.change_number == 0 {
                    self.handle(Msg::PollChanges);
                }
                // Installed Proton builds and runtimes update like games.
                let installed = catalog::scan_installed(&self.settings);
                let tool_ids: Vec<u32> = launch::STEAM_PROTONS
                    .iter()
                    .map(|(id, _)| *id)
                    .chain([launch::SNIPER_APP_ID])
                    .filter(|id| installed.contains_key(id))
                    .collect();
                if !tool_ids.is_empty()
                    && let Some(session) = self.session.clone()
                {
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        if let Ok(apps) = catalog::fetch_apps(&session, &tool_ids).await {
                            let _ = tx.send(Msg::AppsRefreshed(apps));
                        }
                    });
                }
            }
            Msg::PollChanges => {
                let Some(session) = self.session.clone() else {
                    return;
                };
                let since = self.change_number;
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = session
                        .changes_since(since)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(Msg::Changes(result));
                });
            }
            Msg::Changes(Err(e)) => warn!(error = %e, "PICS change poll failed"),
            Msg::Changes(Ok(changes)) => {
                let first = self.change_number == 0;
                self.change_number = changes.current;
                if first {
                    info!(change_number = changes.current, "tracking PICS changes");
                    return;
                }
                let Some(apps) = changes.apps.filter(|_| !changes.packages_changed) else {
                    // Licences changed or Steam asked for a full refresh.
                    self.load_catalog();
                    return;
                };
                let installed: HashSet<u32> = catalog::scan_installed(&self.settings)
                    .into_keys()
                    .collect();
                let relevant: Vec<u32> = apps
                    .into_iter()
                    .filter(|id| installed.contains(id))
                    .collect();
                if relevant.is_empty() {
                    return;
                }
                info!(apps = ?relevant, "installed apps changed upstream");
                let Some(session) = self.session.clone() else {
                    return;
                };
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    match catalog::fetch_apps(&session, &relevant).await {
                        Ok(apps) => {
                            let _ = tx.send(Msg::AppsRefreshed(apps));
                        }
                        Err(e) => warn!(error = %e, "refetching changed apps failed"),
                    }
                });
            }
            Msg::AppsRefreshed(apps) => {
                for app in apps {
                    let id = app.app_id;
                    if let Some(entry) = self.catalog.get_mut(id) {
                        entry.app = app;
                    } else {
                        self.tools.insert(id, app);
                    }
                    self.refresh_install(id);
                }
                self.queue_auto_updates();
                self.publish();
            }
            Msg::CatalogLoaded(Err(e)) => {
                self.emit(UiEvent::Message(format!("Could not load library: {e}")))
            }
            Msg::Progress(app_id, progress) => {
                if let Some(active) = self.active.as_mut().filter(|a| a.app_id == app_id) {
                    if let Progress::Downloading {
                        done,
                        total,
                        bytes_per_sec,
                    } = progress
                    {
                        active.done = done;
                        active.total = total;
                        active.rate = bytes_per_sec;
                    }
                    self.publish();
                }
            }
            Msg::ToolsFetched(Ok(apps)) => {
                for app in apps {
                    let id = app.app_id;
                    self.tools.insert(id, app);
                    self.enqueue(id, false);
                }
            }
            Msg::ToolsFetched(Err(e)) => {
                self.pending_launch = None;
                self.emit(UiEvent::Message(format!("Could not get Proton: {e}")));
            }
            Msg::Exited(app_id, status) => {
                self.running.remove(&app_id);
                info!(app_id, %status, "game exited");
                self.queue_auto_updates();
                self.publish();
            }
            Msg::InstallDone(app_id, result) => {
                self.active = None;
                match result {
                    Ok(()) => info!(app_id, "install finished"),
                    Err(e) if e == "cancelled" => {}
                    Err(e) => {
                        let name = self
                            .catalog
                            .get(app_id)
                            .map(|e| e.app.name.clone())
                            .unwrap_or_default();
                        self.emit(UiEvent::Message(format!("{name}: {e}")));
                    }
                }
                self.refresh_install(app_id);
                self.start_next();
                self.publish();
                self.publish_libraries();
                self.publish_tools();
                if self.active.is_none()
                    && self.queue.is_empty()
                    && let Some(pending) = self.pending_launch.take()
                {
                    self.play(pending);
                }
            }
        }
    }

    fn command(&mut self, cmd: Command) {
        match cmd {
            Command::Login {
                account,
                password,
                remember,
            } => {
                self.remember = remember;
                self.auth(AuthState::Working, "");
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = session::authenticate(&account, &password)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(Msg::Authenticated(result));
                });
            }
            Command::SubmitCode(code) => {
                let Some(challenge) = self.challenge.take() else {
                    return;
                };
                self.auth(AuthState::Working, "");
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = challenge
                        .submit(&code)
                        .await
                        .map_err(|(c, e)| (c, e.to_string()));
                    let _ = tx.send(Msg::CodeResult(result));
                });
            }
            Command::Logout => {
                if let Some(active) = self.active.take() {
                    active.cancel.cancel();
                }
                self.queue.clear();
                if let Some(account) = self.settings.remember_account.take() {
                    config::credentials::forget(&account);
                    let _ = self.settings.save();
                }
                self.session = None;
                self.catalog = Catalog::default();
                self.publish();
                self.auth(AuthState::LoggedOut, "");
            }
            Command::Refresh => self.load_catalog(),
            Command::SetFilter { text, mode } => {
                self.filter_text = text.to_lowercase();
                self.filter_mode = mode;
                self.publish();
            }
            Command::ToggleSection(section) => {
                if !self.collapsed.remove(&section) {
                    self.collapsed.insert(section);
                }
                self.publish();
            }
            Command::Install {
                app_id,
                library,
                runtime,
            } => {
                if let Some(runtime) = runtime {
                    self.set_runtime_choice(app_id, runtime);
                }
                if let Some(index) = library {
                    self.pending_library.insert(app_id, index);
                }
                self.enqueue(app_id, false);
            }
            Command::Update(app_id) => self.enqueue(app_id, false),
            Command::Verify(app_id) => self.enqueue(app_id, true),
            Command::Pause(app_id) => {
                self.queue.retain(|(id, _)| *id != app_id);
                if let Some(active) = self.active.as_ref().filter(|a| a.app_id == app_id) {
                    active.cancel.cancel();
                    self.paused.insert(app_id);
                }
                self.publish();
            }
            Command::Uninstall(app_id) => {
                let Some(entry) = self.catalog.get(app_id) else {
                    return;
                };
                let Some(installed) = &entry.install else {
                    return;
                };
                if self.active.as_ref().is_some_and(|a| a.app_id == app_id) {
                    self.emit(UiEvent::Message(
                        "Pause the download before uninstalling.".into(),
                    ));
                    return;
                }
                let lib = self.settings.libraries[installed.library].clone();
                if let Err(e) = install::uninstall(&lib, app_id, false) {
                    self.emit(UiEvent::Message(format!("Uninstall failed: {e}")));
                }
                self.refresh_install(app_id);
                self.publish();
                self.publish_libraries();
            }
            Command::SetRuntime { app_id, runtime } => {
                self.set_runtime_choice(app_id, runtime);
                let installed_runtime = self
                    .catalog
                    .get(app_id)
                    .and_then(|e| e.install.as_ref())
                    .map(|i| self.installed_runtime(app_id, &i.manifest));
                if installed_runtime.is_some_and(|r| r != runtime) {
                    // Switching an installed game swaps its depots.
                    self.enqueue(app_id, false);
                }
                self.publish();
            }
            Command::SetDlc {
                app_id,
                dlc,
                enabled,
            } => {
                let Some(entry) = self.catalog.get(app_id) else {
                    return;
                };
                if let Some(installed) = &entry.install {
                    let lib = self.settings.libraries[installed.library].clone();
                    let mut manifest = installed.manifest.clone();
                    manifest.disabled_dlc.retain(|d| *d != dlc);
                    if !enabled {
                        manifest.disabled_dlc.push(dlc);
                    }
                    let _ = lib.write_manifest(&manifest);
                    self.refresh_install(app_id);
                    self.enqueue(app_id, false);
                }
                self.publish();
            }
            Command::AddLibrary(path) => {
                let index = self.settings.add_library(&path);
                if let Err(e) = self.settings.libraries[index].ensure_layout() {
                    self.emit(UiEvent::Message(format!(
                        "Cannot use {}: {e}",
                        path.display()
                    )));
                    self.settings.remove_library(index);
                }
                let _ = self.settings.save();
                self.rescan_installed();
            }
            Command::RemoveLibrary(index) => {
                self.settings.remove_library(index);
                let _ = self.settings.save();
                self.rescan_installed();
            }
            Command::SetDefaultLibrary(index) => {
                self.settings.set_default_library(index);
                let _ = self.settings.save();
                self.publish_libraries();
            }
            Command::SetAutoUpdate(on) => {
                self.settings.auto_update = on;
                let _ = self.settings.save();
                self.emit(UiEvent::AutoUpdate(on));
            }
            Command::Play(app_id) => self.play(app_id),
            Command::Stop(app_id) => {
                if let Some(&pgid) = self.running.get(&app_id) {
                    // SAFETY: signalling a process group we spawned; harmless if it has exited.
                    unsafe { libc::kill(-pgid, libc::SIGTERM) };
                }
            }
            Command::SetDefaultProton(key) => {
                self.settings.default_proton = key;
                let _ = self.settings.save();
                self.publish_tools();
                self.publish();
            }
            Command::SetProtonFor { app_id, key } => {
                if key.is_empty() {
                    self.settings.proton_choice.remove(&app_id.to_string());
                } else {
                    self.settings.proton_choice.insert(app_id.to_string(), key);
                }
                let _ = self.settings.save();
                self.publish();
            }
            Command::SetLaunchOptions { app_id, options } => {
                if options.trim().is_empty() {
                    self.settings.launch_options.remove(&app_id.to_string());
                } else {
                    self.settings
                        .launch_options
                        .insert(app_id.to_string(), options.trim().to_string());
                }
                let _ = self.settings.save();
                self.publish();
            }
        }
    }

    fn set_runtime_choice(&mut self, app_id: u32, runtime: Runtime) {
        self.settings
            .runtime_choice
            .insert(app_id.to_string(), runtime.as_str().to_string());
        let _ = self.settings.save();
    }

    fn load_catalog(&mut self) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let settings = self.settings.clone();
        let tx = self.tx.clone();
        let demo = self.demo.clone();
        self.emit(UiEvent::Footer("Loading library…".into()));
        tokio::spawn(async move {
            let result = match demo {
                Some(apps) => catalog::fetch_apps(&session, &apps).await.map(|details| {
                    Catalog::assemble(
                        details,
                        apps.iter().copied().collect(),
                        catalog::scan_installed(&settings),
                    )
                }),
                None => Catalog::load(&session, &settings).await,
            }
            .map_err(|e| e.to_string());
            let _ = tx.send(Msg::CatalogLoaded(result));
        });
    }

    /// Re-read install state from disk after library folders change.
    fn rescan_installed(&mut self) {
        let installed = catalog::scan_installed(&self.settings);
        for entry in &mut self.catalog.entries {
            entry.install =
                installed
                    .get(&entry.app.app_id)
                    .map(|(library, manifest)| catalog::Installed {
                        library: *library,
                        update_available: install::needs_update(&entry.app, manifest),
                        manifest: manifest.clone(),
                    });
        }
        self.publish();
        self.publish_libraries();
    }

    fn refresh_install(&mut self, app_id: u32) {
        let installed = catalog::scan_installed(&self.settings);
        if let Some(entry) = self.catalog.get_mut(app_id) {
            entry.install = installed
                .get(&app_id)
                .map(|(library, manifest)| catalog::Installed {
                    library: *library,
                    update_available: install::needs_update(&entry.app, manifest),
                    manifest: manifest.clone(),
                });
        }
    }

    fn installed_runtime(&self, _app_id: u32, manifest: &library::AppManifest) -> Runtime {
        manifest
            .runtime
            .as_deref()
            .and_then(Runtime::parse)
            .unwrap_or_else(|| {
                let linux = self.catalog.get(manifest.app_id).is_some_and(|e| {
                    e.app.depots.iter().any(|d| {
                        manifest.installed_depots.contains_key(&d.id)
                            && d.oslist.iter().any(|o| o == "linux")
                    })
                });
                if linux {
                    Runtime::Native
                } else {
                    Runtime::Proton
                }
            })
    }

    fn chosen_runtime(&self, entry: &Entry) -> Runtime {
        if let Some(installed) = &entry.install
            && !self
                .settings
                .runtime_choice
                .contains_key(&entry.app.app_id.to_string())
        {
            return self.installed_runtime(entry.app.app_id, &installed.manifest);
        }
        let preferred = Runtime::preferred_for(&entry.app);
        let chosen = self
            .settings
            .runtime_choice
            .get(&entry.app.app_id.to_string())
            .and_then(|s| Runtime::parse(s))
            .unwrap_or(preferred);
        match (
            chosen,
            entry.app.supports_linux(),
            entry.app.supports_windows(),
        ) {
            (Runtime::Native, false, _) => Runtime::Proton,
            (Runtime::Proton, true, false) => Runtime::Native,
            _ => chosen,
        }
    }

    fn enqueue(&mut self, app_id: u32, verify: bool) {
        self.paused.remove(&app_id);
        let busy = self.active.as_ref().is_some_and(|a| a.app_id == app_id);
        if !busy && !self.queue.iter().any(|(id, _)| *id == app_id) {
            self.queue.push_back((app_id, verify));
        }
        self.start_next();
        self.publish();
    }

    fn start_next(&mut self) {
        if self.active.is_some() {
            return;
        }
        let Some(session) = self.session.clone() else {
            return;
        };
        while let Some((app_id, verify)) = self.queue.pop_front() {
            let tool_entry;
            let entry = match self.catalog.get(app_id) {
                Some(e) => e,
                None => {
                    // Tool apps install like games, into the library they're already in or the default.
                    let Some(app) = self.tools.get(&app_id).cloned() else {
                        continue;
                    };
                    let installed = catalog::scan_installed(&self.settings).remove(&app_id);
                    tool_entry = Entry {
                        app,
                        install: installed.map(|(library, manifest)| catalog::Installed {
                            library,
                            manifest,
                            update_available: true,
                        }),
                        dlc: Vec::new(),
                    };
                    &tool_entry
                }
            };
            let library = match &entry.install {
                Some(i) => self.settings.libraries.get(i.library).cloned(),
                None => self
                    .pending_library
                    .remove(&app_id)
                    .and_then(|i| self.settings.libraries.get(i).cloned())
                    .or_else(|| self.settings.default_library().cloned()),
            };
            let Some(library) = library else {
                self.emit(UiEvent::Message(
                    "Add a library folder in Settings first.".into(),
                ));
                continue;
            };
            let installed = entry.install.as_ref().map(|i| &i.manifest);
            let req = InstallRequest {
                app: entry.app.clone(),
                library,
                runtime: if self.tools.contains_key(&app_id) {
                    Runtime::Native
                } else {
                    self.chosen_runtime(entry)
                },
                branch: installed
                    .and_then(|m| m.beta_key.clone())
                    .unwrap_or_else(|| "public".into()),
                language: self.settings.language.clone(),
                owned: self.catalog.owned.clone(),
                disabled_dlc: installed
                    .map(|m| m.disabled_dlc.clone())
                    .unwrap_or_default(),
                verify,
            };
            let cancel = CancellationToken::new();
            self.active = Some(ActiveDownload {
                app_id,
                cancel: cancel.clone(),
                done: 0,
                total: 0,
                rate: 0,
            });
            let tx = self.tx.clone();
            let session = session.clone();
            tokio::spawn(async move {
                let (ptx, mut prx) = mpsc::unbounded_channel();
                let forward = {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        while let Some(p) = prx.recv().await {
                            let _ = tx.send(Msg::Progress(app_id, p));
                        }
                    })
                };
                let result = install::install(&session, &req, ptx, cancel)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string());
                if let Err(e) = &result {
                    error!(app_id, error = %e, "install failed");
                }
                let _ = forward.await;
                let _ = tx.send(Msg::InstallDone(app_id, result));
            });
            return;
        }
    }

    /// Queue updates for installed games (and tool apps) behind their branch, when automatic
    /// updates are on. Running games are left alone until they exit.
    fn queue_auto_updates(&mut self) {
        if !self.settings.auto_update {
            return;
        }
        let mut pending: Vec<u32> = self
            .catalog
            .entries
            .iter()
            .filter(|e| e.install.as_ref().is_some_and(|i| i.update_available))
            .map(Entry::app_id)
            .collect();
        let installed = catalog::scan_installed(&self.settings);
        for (id, app) in &self.tools {
            if let Some((_, manifest)) = installed.get(id)
                && install::needs_update(app, manifest)
            {
                pending.push(*id);
            }
        }
        for app_id in pending {
            if !self.running.contains_key(&app_id) {
                self.enqueue(app_id, false);
            }
        }
    }

    fn proton_key_for(&self, app_id: u32) -> String {
        self.settings
            .proton_choice
            .get(&app_id.to_string())
            .cloned()
            .unwrap_or_else(|| self.settings.default_proton.clone())
    }

    fn tool_name(&self, key: &str) -> String {
        launch::STEAM_PROTONS
            .iter()
            .find(|(id, _)| id.to_string() == key)
            .map(|(_, n)| n.to_string())
            .or_else(|| {
                launch::installed_tools(&self.settings)
                    .into_iter()
                    .find(|t| t.key == key)
                    .map(|t| t.name)
            })
            .unwrap_or_else(|| key.trim_start_matches("custom:").to_string())
    }

    /// Start a game, first fetching anything it needs: a pending update, the Proton build and
    /// the Steam Linux Runtime it requires.
    fn play(&mut self, app_id: u32) {
        if self.running.contains_key(&app_id) {
            return;
        }
        let Some(entry) = self.catalog.get(app_id).cloned() else {
            return;
        };
        let Some(installed) = entry.install.clone() else {
            self.emit(UiEvent::Message(format!(
                "{} is not installed.",
                entry.app.name
            )));
            return;
        };
        if installed.update_available || installed.manifest.needs_update() {
            self.pending_launch = Some(app_id);
            self.enqueue(app_id, false);
            return;
        }
        let runtime = self.chosen_runtime(&entry);
        let mut tool: Option<CompatTool> = None;
        if runtime == Runtime::Proton {
            let key = self.proton_key_for(app_id);
            let tools = launch::installed_tools(&self.settings);
            let mut missing = Vec::new();
            match tools.into_iter().find(|t| t.key == key) {
                Some(t) => {
                    if let Some(req) =
                        launch::read_toolmanifest(&t.dir).and_then(|m| m.require_tool_appid)
                        && launch::installed_app_dir(&self.settings, req).is_none()
                    {
                        missing.push(req);
                    }
                    tool = Some(t);
                }
                None => match key.parse::<u32>() {
                    Ok(id) => {
                        missing.push(id);
                        if launch::installed_app_dir(&self.settings, launch::SNIPER_APP_ID)
                            .is_none()
                        {
                            missing.push(launch::SNIPER_APP_ID);
                        }
                    }
                    Err(_) => {
                        self.emit(UiEvent::Message(format!(
                            "{} is not installed.",
                            self.tool_name(&key)
                        )));
                        return;
                    }
                },
            }
            if !missing.is_empty() {
                self.pending_launch = Some(app_id);
                self.emit(UiEvent::Message(format!(
                    "Installing {} before launch…",
                    self.tool_name(&key)
                )));
                let Some(session) = self.session.clone() else {
                    return;
                };
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = catalog::fetch_apps(&session, &missing)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(Msg::ToolsFetched(result));
                });
                return;
            }
        }

        let Some(library) = self.settings.libraries.get(installed.library).cloned() else {
            return;
        };
        let options = self
            .settings
            .launch_options
            .get(&app_id.to_string())
            .cloned()
            .unwrap_or_default();
        let ctx = launch::LaunchContext {
            settings: &self.settings,
            app: &entry.app,
            manifest: &installed.manifest,
            library: &library,
            runtime,
            tool: tool.as_ref(),
            launch_options: &options,
        };
        let plan = match launch::plan(&ctx) {
            Ok(p) => p,
            Err(e) => {
                self.emit(UiEvent::Message(format!(
                    "Cannot start {}: {e}",
                    entry.app.name
                )));
                return;
            }
        };
        match spawn_game(app_id, &plan, self.tx.clone()) {
            Ok(pgid) => {
                info!(app_id, program = %plan.program.display(), "game started");
                self.running.insert(app_id, pgid);
            }
            Err(e) => self.emit(UiEvent::Message(format!(
                "Cannot start {}: {e}",
                entry.app.name
            ))),
        }
        self.publish();
    }

    fn publish_tools(&self) {
        let installed = launch::installed_tools(&self.settings);
        let mut tools: Vec<ToolInfo> = launch::STEAM_PROTONS
            .iter()
            .map(|(id, name)| ToolInfo {
                key: id.to_string(),
                name: name.to_string(),
                installed: installed.iter().any(|t| t.key == id.to_string()),
            })
            .collect();
        tools.extend(
            installed
                .iter()
                .filter(|t| t.key.starts_with("custom:"))
                .map(|t| ToolInfo {
                    key: t.key.clone(),
                    name: t.name.clone(),
                    installed: true,
                }),
        );
        self.emit(UiEvent::Tools {
            tools,
            default: self.settings.default_proton.clone(),
        });
    }

    fn planned_size(&mut self, entry: &Entry, runtime: Runtime) -> u64 {
        let key = (entry.app.app_id, runtime);
        if let Some(size) = self.install_size_cache.get(&key) {
            return *size;
        }
        let req = InstallRequest {
            app: entry.app.clone(),
            library: LibraryFolder::new(PathBuf::new()),
            runtime,
            branch: "public".into(),
            language: self.settings.language.clone(),
            owned: self.catalog.owned.clone(),
            disabled_dlc: Vec::new(),
            verify: false,
        };
        let size = install::plan(&req)
            .map(|p| p.iter().map(|d| d.manifest.size).sum())
            .unwrap_or(0);
        self.install_size_cache.insert(key, size);
        size
    }

    fn matches_filter(&self, entry: &Entry) -> bool {
        let os_ok = match self.filter_mode {
            Filter::All => true,
            Filter::Native => entry.app.supports_linux(),
            Filter::Proton => entry.app.supports_windows(),
        };
        os_ok
            && (self.filter_text.is_empty()
                || entry.app.name.to_lowercase().contains(&self.filter_text))
    }

    fn game_row(&mut self, index: usize) -> Row {
        let entry = self.catalog.entries[index].clone();
        let app = &entry.app;
        let runtime = self.chosen_runtime(&entry);
        let os = match (app.supports_linux(), app.supports_windows()) {
            (true, true) => "both",
            (true, false) => "linux",
            _ => "windows",
        };
        let planned = if entry.install.is_none() {
            self.planned_size(&entry, runtime)
        } else {
            0
        };
        let queued = self.queue.iter().any(|(id, _)| *id == app.app_id);
        let active = self.active.as_ref().filter(|a| a.app_id == app.app_id);
        let mut row = Row {
            section: if entry.install.is_some() {
                "installed"
            } else {
                "available"
            }
            .into(),
            app_id: app.app_id,
            title: app.name.clone(),
            os: os.into(),
            runtime: runtime.as_str().into(),
            icon_url: app.icon_url().unwrap_or_default(),
            ..Default::default()
        };
        let dlc: Vec<serde_json::Value> = entry
            .dlc
            .iter()
            .map(|d| {
                let disabled = entry.install.as_ref().is_some_and(|i| i.manifest.disabled_dlc.contains(&d.app_id));
                serde_json::json!({ "id": d.app_id, "name": d.name, "enabled": !disabled, "content": d.has_content })
            })
            .collect();
        row.dlc_json = serde_json::Value::Array(dlc).to_string();
        row.proton_key = self
            .settings
            .proton_choice
            .get(&app.app_id.to_string())
            .cloned()
            .unwrap_or_default();
        row.proton = self.tool_name(&self.proton_key_for(app.app_id));
        row.launch_options = self
            .settings
            .launch_options
            .get(&app.app_id.to_string())
            .cloned()
            .unwrap_or_default();
        let running = self.running.contains_key(&app.app_id);

        match &entry.install {
            Some(installed) => {
                let m = &installed.manifest;
                row.size = human_size(m.size_on_disk);
                row.build = m.build_id.to_string();
                row.branch = m.beta_key.clone().unwrap_or_else(|| "public".into());
                row.updated = date(m.last_updated);
                row.location = self
                    .settings
                    .libraries
                    .get(installed.library)
                    .map(|l| l.install_path(&m.install_dir).display().to_string())
                    .unwrap_or_default();
                let (state, status) = if running {
                    ("running", "Running".to_string())
                } else if let Some(a) = active {
                    row.progress = if a.total > 0 {
                        a.done as f64 / a.total as f64
                    } else {
                        0.0
                    };
                    let eta = a
                        .total
                        .saturating_sub(a.done)
                        .checked_div(a.rate)
                        .map_or_else(|| "…".into(), human_eta);
                    (
                        "downloading",
                        format!("Updating · {}/s · {eta} left", human_size(a.rate)),
                    )
                } else if queued {
                    ("queued", "Update queued".to_string())
                } else if self.paused.contains(&app.app_id) || m.needs_update() {
                    ("paused", "Update paused".to_string())
                } else if installed.update_available {
                    ("update", "Update available".to_string())
                } else {
                    ("ready", "Up to date".to_string())
                };
                row.state = state.into();
                row.status = status;
            }
            None => {
                row.size = human_size(planned);
                row.status = match os {
                    "both" => "Native · Proton".into(),
                    "linux" => "Native".into(),
                    _ => "Proton".into(),
                };
                if let Some(a) = active {
                    row.state = "downloading".into();
                    row.progress = if a.total > 0 {
                        a.done as f64 / a.total as f64
                    } else {
                        0.0
                    };
                    row.status = format!("Installing · {}/s", human_size(a.rate));
                } else if queued {
                    row.state = "queued".into();
                    row.status = "Queued".into();
                } else {
                    row.state = "none".into();
                }
            }
        }
        row
    }

    fn publish(&mut self) {
        let mut installed = Vec::new();
        let mut available = Vec::new();
        for i in 0..self.catalog.entries.len() {
            if !self.matches_filter(&self.catalog.entries[i]) {
                continue;
            }
            if self.catalog.entries[i].install.is_some() {
                installed.push(i);
            } else {
                available.push(i);
            }
        }
        let mut rows = Vec::new();
        for (section, title, items) in [
            ("installed", "Installed", installed),
            ("available", "Not installed", available),
        ] {
            let expanded = !self.collapsed.contains(section);
            let extra = if section == "installed" {
                let total: u64 = items
                    .iter()
                    .filter_map(|&i| self.catalog.entries[i].install.as_ref())
                    .map(|i| i.manifest.size_on_disk)
                    .sum();
                human_size(total)
            } else {
                format!("{} games", items.len())
            };
            rows.push(Row {
                header: true,
                section: section.into(),
                title: format!("{title} ({})", items.len()),
                extra,
                expanded,
                ..Default::default()
            });
            if expanded {
                for i in items {
                    rows.push(self.game_row(i));
                }
            }
        }
        self.emit(UiEvent::Rows(rows));
        self.publish_footer();
    }

    fn publish_footer(&self) {
        let text = match &self.active {
            Some(a) => {
                let name = self
                    .catalog
                    .get(a.app_id)
                    .map(|e| e.app.name.as_str())
                    .unwrap_or("");
                let queued = self.queue.len();
                let queued = if queued > 0 {
                    format!(" · {queued} queued")
                } else {
                    String::new()
                };
                format!("Downloading {name}{queued} · {}/s", human_size(a.rate))
            }
            None if !self.queue.is_empty() => format!("{} queued", self.queue.len()),
            None => {
                let n = self.catalog.entries.len();
                if n == 0 {
                    String::new()
                } else {
                    format!("{n} games in library")
                }
            }
        };
        self.emit(UiEvent::Footer(text));
    }

    fn publish_libraries(&self) {
        let installed = catalog::scan_installed(&self.settings);
        let externals: Vec<PathBuf> = library::detect_official_steam()
            .into_iter()
            .flat_map(|s| s.libraries)
            .collect();
        let mut out: Vec<LibraryInfo> = self
            .settings
            .libraries
            .iter()
            .enumerate()
            .map(|(i, lib)| {
                let space = lib.disk_space().unwrap_or_default();
                LibraryInfo {
                    path: lib.path.display().to_string(),
                    default: Some(lib) == self.settings.default_library(),
                    games: installed.values().filter(|(l, _)| *l == i).count(),
                    free: space.free,
                    total: space.total,
                    external: false,
                }
            })
            .collect();
        for path in externals {
            if self.settings.libraries.iter().any(|l| l.path == path) {
                continue;
            }
            let lib = LibraryFolder::new(&path);
            let space = lib.disk_space().unwrap_or_default();
            out.push(LibraryInfo {
                path: path.display().to_string(),
                default: false,
                games: lib.manifests().len(),
                free: space.free,
                total: space.total,
                external: true,
            });
        }
        self.emit(UiEvent::Libraries(out));
    }
}

/// Start a game in its own process group (so Stop reaches child processes too), logging its
/// output to `<data>/logs/<appid>.log`.
fn spawn_game(
    app_id: u32,
    plan: &launch::LaunchPlan,
    tx: mpsc::UnboundedSender<Msg>,
) -> std::io::Result<i32> {
    use std::os::unix::process::CommandExt;
    let logs = config::data_dir().join("logs");
    std::fs::create_dir_all(&logs)?;
    let log = std::fs::File::create(logs.join(format!("{app_id}.log")))?;
    let mut cmd = plan.command();
    cmd.process_group(0)
        .stdout(log.try_clone()?)
        .stderr(log)
        .stdin(std::process::Stdio::null());
    let mut child = tokio::process::Command::from(cmd).spawn()?;
    let pgid = child.id().unwrap_or(0) as i32;
    tokio::spawn(async move {
        let status = match child.wait().await {
            Ok(s) => s.to_string(),
            Err(e) => e.to_string(),
        };
        let _ = tx.send(Msg::Exited(app_id, status));
    });
    Ok(pgid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting() {
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(12_400_000_000), "12.4 GB");
        assert_eq!(human_size(185_700_000_000), "186 GB");
        assert_eq!(human_eta(42), "0:42");
        assert_eq!(human_eta(3725), "1:02:05");
        assert_eq!(date(1761177600), "2025-10-23");
    }
}
