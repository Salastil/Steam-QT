use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("org.steamqt").qml_files([
        "qml/Main.qml",
        "qml/LoginPage.qml",
        "qml/LibraryPage.qml",
        "qml/SettingsPage.qml",
        "qml/GameRow.qml",
        "qml/SectionHeader.qml",
        "qml/Segmented.qml",
        "qml/InstallDialog.qml",
    ]))
    .qt_module("Widgets")
    .qt_module("Quick")
    .files(["src/bridge.rs"])
    .cpp_file("src/app.cpp")
    .build();
}
