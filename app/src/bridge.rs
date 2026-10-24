//! Qt bridge: the `Library` QML type, a list model plus the backend's properties and actions.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++Qt" {
        include!(<QtCore/QAbstractListModel>);
        #[qobject]
        type QAbstractListModel;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvector.h");
        type QVector_i32 = cxx_qt_lib::QVector<i32>;
    }

    extern "RustQt" {
        #[qobject]
        #[base = QAbstractListModel]
        #[qml_element]
        #[qproperty(QString, auth_state, cxx_name = "authState")]
        #[qproperty(QString, auth_error, cxx_name = "authError")]
        #[qproperty(QString, account)]
        #[qproperty(QString, footer)]
        #[qproperty(bool, auto_update, cxx_name = "autoUpdate")]
        #[qproperty(QString, libraries_json, cxx_name = "librariesJson")]
        #[qproperty(QString, tools_json, cxx_name = "toolsJson")]
        #[qproperty(QString, default_proton, cxx_name = "defaultProton")]
        type Library = super::LibraryRust;
    }

    impl cxx_qt::Threading for Library {}
    impl cxx_qt::Initialize for Library {}

    extern "RustQt" {
        /// A one-off notice for the user (errors, hints).
        #[qsignal]
        fn message(self: Pin<&mut Library>, text: QString);
    }

    unsafe extern "RustQt" {
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut Library>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );

        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut Library>);
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut Library>);

        #[inherit]
        fn index(self: &Library, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;
    }

    extern "RustQt" {
        #[qinvokable]
        #[cxx_override]
        fn data(self: &Library, index: &QModelIndex, role: i32) -> QVariant;
        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &Library) -> QHash_i32_QByteArray;
        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &Library, parent: &QModelIndex) -> i32;
    }

    extern "RustQt" {
        #[qinvokable]
        fn login(self: &Library, account: &QString, password: &QString, remember: bool);
        #[qinvokable]
        #[cxx_name = "submitCode"]
        fn submit_code(self: &Library, code: &QString);
        #[qinvokable]
        fn logout(self: &Library);
        #[qinvokable]
        fn refresh(self: &Library);
        #[qinvokable]
        #[cxx_name = "setFilter"]
        fn set_filter(self: &Library, text: &QString, mode: &QString);
        #[qinvokable]
        #[cxx_name = "toggleSection"]
        fn toggle_section(self: &Library, section: &QString);
        /// `library` is an index into the libraries list or -1 for the default; `runtime` is
        /// "native", "proton" or empty for the current choice.
        #[qinvokable]
        fn install(self: &Library, app_id: u32, library: i32, runtime: &QString);
        #[qinvokable]
        fn update(self: &Library, app_id: u32);
        #[qinvokable]
        fn verify(self: &Library, app_id: u32);
        #[qinvokable]
        fn pause(self: &Library, app_id: u32);
        #[qinvokable]
        fn uninstall(self: &Library, app_id: u32);
        #[qinvokable]
        fn play(self: &Library, app_id: u32);
        #[qinvokable]
        #[cxx_name = "setRuntime"]
        fn set_runtime(self: &Library, app_id: u32, runtime: &QString);
        #[qinvokable]
        #[cxx_name = "setDlc"]
        fn set_dlc(self: &Library, app_id: u32, dlc: u32, enabled: bool);
        /// Accepts a local path or a `file://` URL (from a folder dialog).
        #[qinvokable]
        #[cxx_name = "addLibrary"]
        fn add_library(self: &Library, path: &QString);
        #[qinvokable]
        #[cxx_name = "removeLibrary"]
        fn remove_library(self: &Library, index: i32);
        #[qinvokable]
        #[cxx_name = "setDefaultLibrary"]
        fn set_default_library(self: &Library, index: i32);
        #[qinvokable]
        #[cxx_name = "setAutoUpdateEnabled"]
        fn set_auto_update_enabled(self: &Library, on: bool);
        #[qinvokable]
        fn stop(self: &Library, app_id: u32);
        #[qinvokable]
        #[cxx_name = "selectDefaultProton"]
        fn select_default_proton(self: &Library, key: &QString);
        /// Empty `key` returns the game to the default tool.
        #[qinvokable]
        #[cxx_name = "setProtonFor"]
        fn set_proton_for(self: &Library, app_id: u32, key: &QString);
        #[qinvokable]
        #[cxx_name = "setLaunchOptions"]
        fn set_launch_options(self: &Library, app_id: u32, options: &QString);
    }
}

use crate::worker::{self, Command, Filter, Row, UiEvent};
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant, QVector,
};
use std::path::PathBuf;
use std::sync::Arc;
use steamqt_core::install::Runtime;

const ROLES: &[&str] = &[
    "header",
    "section",
    "title",
    "extra",
    "expanded",
    "appId",
    "os",
    "runtime",
    "gameState",
    "status",
    "size",
    "progress",
    "iconUrl",
    "build",
    "branch",
    "updated",
    "location",
    "dlcJson",
    "proton",
    "protonKey",
    "launchOptions",
];
const USER_ROLE: i32 = 0x0100;

#[derive(Default)]
pub struct LibraryRust {
    auth_state: QString,
    auth_error: QString,
    account: QString,
    footer: QString,
    auto_update: bool,
    libraries_json: QString,
    tools_json: QString,
    default_proton: QString,
    rows: Vec<Row>,
    worker: Option<worker::Handle>,
}

impl cxx_qt::Initialize for qobject::Library {
    fn initialize(mut self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        let notify: worker::Notify = Arc::new(move |event| {
            let _ = qt.queue(move |obj| obj.apply(event));
        });
        self.as_mut().rust_mut().worker = Some(worker::spawn(notify));
    }
}

fn row_value(row: &Row, role: usize) -> QVariant {
    let s = |v: &str| QVariant::from(&QString::from(v));
    match ROLES.get(role).copied() {
        Some("header") => QVariant::from(&row.header),
        Some("section") => s(&row.section),
        Some("title") => s(&row.title),
        Some("extra") => s(&row.extra),
        Some("expanded") => QVariant::from(&row.expanded),
        Some("appId") => QVariant::from(&row.app_id),
        Some("os") => s(&row.os),
        Some("runtime") => s(&row.runtime),
        Some("gameState") => s(&row.state),
        Some("status") => s(&row.status),
        Some("size") => s(&row.size),
        Some("progress") => QVariant::from(&row.progress),
        Some("iconUrl") => s(&row.icon_url),
        Some("build") => s(&row.build),
        Some("branch") => s(&row.branch),
        Some("updated") => s(&row.updated),
        Some("location") => s(&row.location),
        Some("dlcJson") => s(&row.dlc_json),
        Some("proton") => s(&row.proton),
        Some("protonKey") => s(&row.proton_key),
        Some("launchOptions") => s(&row.launch_options),
        _ => QVariant::default(),
    }
}

fn same_shape(a: &[Row], b: &[Row]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.header == y.header && x.app_id == y.app_id && x.section == y.section)
}

impl qobject::Library {
    fn send(&self, cmd: Command) {
        if let Some(w) = &self.worker {
            w.send(cmd);
        }
    }

    fn apply(mut self: Pin<&mut Self>, event: UiEvent) {
        match event {
            UiEvent::Auth { state, error } => {
                self.as_mut().set_auth_error(QString::from(error.as_str()));
                self.as_mut().set_auth_state(QString::from(state.as_str()));
            }
            UiEvent::Account(a) => self.as_mut().set_account(QString::from(a.as_str())),
            UiEvent::Footer(f) => self.as_mut().set_footer(QString::from(f.as_str())),
            UiEvent::AutoUpdate(on) => self.as_mut().set_auto_update(on),
            UiEvent::Message(m) => self.as_mut().message(QString::from(m.as_str())),
            UiEvent::Libraries(libs) => {
                let json = serde_json::to_string(&libs).unwrap_or_else(|_| "[]".into());
                self.as_mut()
                    .set_libraries_json(QString::from(json.as_str()));
            }
            UiEvent::Rows(rows) => self.set_rows(rows),
            UiEvent::Tools { tools, default } => {
                let json = serde_json::to_string(&tools).unwrap_or_else(|_| "[]".into());
                self.as_mut().set_tools_json(QString::from(json.as_str()));
                self.as_mut()
                    .set_default_proton(QString::from(default.as_str()));
            }
        }
    }

    /// Update in place when only row contents changed (keeps scroll position and open menus);
    /// reset the model when rows were added, removed or reordered.
    fn set_rows(mut self: Pin<&mut Self>, rows: Vec<Row>) {
        if same_shape(&self.rows, &rows) {
            let changed: Vec<usize> = (0..rows.len())
                .filter(|&i| self.rows[i] != rows[i])
                .collect();
            self.as_mut().rust_mut().rows = rows;
            let roles = QVector::<i32>::default();
            for i in changed {
                let idx = self.index(i as i32, 0, &QModelIndex::default());
                self.as_mut().data_changed(&idx, &idx, &roles);
            }
        } else {
            // SAFETY: begin/end are paired and the row storage is replaced in between.
            unsafe {
                self.as_mut().begin_reset_model();
                self.as_mut().rust_mut().rows = rows;
                self.as_mut().end_reset_model();
            }
        }
    }

    pub fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        match self.rows.get(index.row() as usize) {
            Some(row) if role >= USER_ROLE => row_value(row, (role - USER_ROLE) as usize),
            _ => QVariant::default(),
        }
    }

    pub fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        for (i, name) in ROLES.iter().enumerate() {
            roles.insert(USER_ROLE + i as i32, QByteArray::from(*name));
        }
        roles
    }

    pub fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rows.len() as i32
    }

    pub fn login(&self, account: &QString, password: &QString, remember: bool) {
        self.send(Command::Login {
            account: account.to_string(),
            password: password.to_string(),
            remember,
        });
    }

    pub fn submit_code(&self, code: &QString) {
        self.send(Command::SubmitCode(code.to_string()));
    }

    pub fn logout(&self) {
        self.send(Command::Logout);
    }

    pub fn refresh(&self) {
        self.send(Command::Refresh);
    }

    pub fn set_filter(&self, text: &QString, mode: &QString) {
        let mode = match mode.to_string().as_str() {
            "Native" => Filter::Native,
            "Proton" => Filter::Proton,
            _ => Filter::All,
        };
        self.send(Command::SetFilter {
            text: text.to_string(),
            mode,
        });
    }

    pub fn toggle_section(&self, section: &QString) {
        self.send(Command::ToggleSection(section.to_string()));
    }

    pub fn install(&self, app_id: u32, library: i32, runtime: &QString) {
        self.send(Command::Install {
            app_id,
            library: usize::try_from(library).ok(),
            runtime: Runtime::parse(&runtime.to_string()),
        });
    }

    pub fn update(&self, app_id: u32) {
        self.send(Command::Update(app_id));
    }

    pub fn verify(&self, app_id: u32) {
        self.send(Command::Verify(app_id));
    }

    pub fn pause(&self, app_id: u32) {
        self.send(Command::Pause(app_id));
    }

    pub fn uninstall(&self, app_id: u32) {
        self.send(Command::Uninstall(app_id));
    }

    pub fn play(&self, app_id: u32) {
        self.send(Command::Play(app_id));
    }

    pub fn set_runtime(&self, app_id: u32, runtime: &QString) {
        if let Some(runtime) = Runtime::parse(&runtime.to_string()) {
            self.send(Command::SetRuntime { app_id, runtime });
        }
    }

    pub fn set_dlc(&self, app_id: u32, dlc: u32, enabled: bool) {
        self.send(Command::SetDlc {
            app_id,
            dlc,
            enabled,
        });
    }

    pub fn add_library(&self, path: &QString) {
        let text = path.to_string();
        let path = match url::Url::parse(&text) {
            Ok(url) if url.scheme() == "file" => {
                url.to_file_path().unwrap_or_else(|_| PathBuf::from(&text))
            }
            _ => PathBuf::from(text),
        };
        self.send(Command::AddLibrary(path));
    }

    pub fn remove_library(&self, index: i32) {
        if let Ok(i) = usize::try_from(index) {
            self.send(Command::RemoveLibrary(i));
        }
    }

    pub fn set_default_library(&self, index: i32) {
        if let Ok(i) = usize::try_from(index) {
            self.send(Command::SetDefaultLibrary(i));
        }
    }

    pub fn set_auto_update_enabled(&self, on: bool) {
        self.send(Command::SetAutoUpdate(on));
    }

    pub fn stop(&self, app_id: u32) {
        self.send(Command::Stop(app_id));
    }

    pub fn select_default_proton(&self, key: &QString) {
        self.send(Command::SetDefaultProton(key.to_string()));
    }

    pub fn set_proton_for(&self, app_id: u32, key: &QString) {
        self.send(Command::SetProtonFor {
            app_id,
            key: key.to_string(),
        });
    }

    pub fn set_launch_options(&self, app_id: u32, options: &QString) {
        self.send(Command::SetLaunchOptions {
            app_id,
            options: options.to_string(),
        });
    }
}
