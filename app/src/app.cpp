// Application entry point. A QApplication (rather than QGuiApplication) is required for the
// KDE Plasma platform theme and the org.kde.desktop Qt Quick style, which give Breeze controls
// and follow the system colour scheme, including light/dark switching.

#include <QtCore/QUrl>
#include <QtGui/QIcon>
#include <QtGui/QPalette>
#include <QtQml/QQmlApplicationEngine>
#include <QtWidgets/QApplication>

namespace {

// Breeze Dark, used only when STEAMQT_PALETTE=dark is set (for testing outside Plasma).
QPalette breezeDark()
{
    QPalette p;
    p.setColor(QPalette::Window, QColor(0x20, 0x23, 0x26));
    p.setColor(QPalette::WindowText, QColor(0xfc, 0xfc, 0xfc));
    p.setColor(QPalette::Base, QColor(0x14, 0x16, 0x18));
    p.setColor(QPalette::AlternateBase, QColor(0x1d, 0x1f, 0x22));
    p.setColor(QPalette::Text, QColor(0xfc, 0xfc, 0xfc));
    p.setColor(QPalette::Button, QColor(0x29, 0x2c, 0x30));
    p.setColor(QPalette::ButtonText, QColor(0xfc, 0xfc, 0xfc));
    p.setColor(QPalette::Highlight, QColor(0x3d, 0xae, 0xe9));
    p.setColor(QPalette::HighlightedText, QColor(0xfc, 0xfc, 0xfc));
    p.setColor(QPalette::Mid, QColor(0x3b, 0x40, 0x45));
    p.setColor(QPalette::Light, QColor(0x3b, 0x40, 0x45));
    p.setColor(QPalette::Dark, QColor(0x0f, 0x10, 0x12));
    p.setColor(QPalette::PlaceholderText, QColor(0xa1, 0xa9, 0xb1));
    p.setColor(QPalette::ToolTipBase, QColor(0x29, 0x2c, 0x30));
    p.setColor(QPalette::ToolTipText, QColor(0xfc, 0xfc, 0xfc));
    return p;
}

} // namespace

extern "C" int steamqt_run(int argc, char** argv)
{
    static int s_argc = argc;
    QApplication::setApplicationName(QStringLiteral("steam-qt"));
    QApplication::setApplicationDisplayName(QStringLiteral("Steam-QT"));
    QApplication::setOrganizationName(QStringLiteral("steam-qt"));
    QGuiApplication::setDesktopFileName(QStringLiteral("org.steamqt.SteamQt"));
    QApplication app(s_argc, argv);
    app.setWindowIcon(QIcon::fromTheme(QStringLiteral("steam"), QIcon::fromTheme(QStringLiteral("applications-games"))));
    if (qgetenv("STEAMQT_PALETTE") == "dark") {
        app.setPalette(breezeDark());
    }

    QQmlApplicationEngine engine;
    QObject::connect(
        &engine, &QQmlApplicationEngine::objectCreationFailed, &app,
        [] { QCoreApplication::exit(1); }, Qt::QueuedConnection);
    engine.load(QUrl(QStringLiteral("qrc:/qt/qml/org/steamqt/qml/Main.qml")));
    return app.exec();
}
