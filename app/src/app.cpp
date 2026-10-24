// Application entry point. A QApplication (rather than QGuiApplication) is required for the
// KDE Plasma platform theme and the org.kde.desktop Qt Quick style, which give Breeze controls
// and follow the system colour scheme, including light/dark switching.

#include <memory>

#include <QtCore/QDir>
#include <QtCore/QFile>
#include <QtCore/QStandardPaths>
#include <QtCore/QTimer>
#include <QtCore/QUrl>
#include <QtGui/QIcon>
#include <QtGui/QPalette>
#include <QtGui/QPixmap>
#include <QtQml/QQmlApplicationEngine>
#include <QtQuick/QQuickWindow>
#include <QtWidgets/QApplication>
#include <QtWidgets/QMenu>
#include <QtWidgets/QSystemTrayIcon>

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

const char* kTrayIconName = "steam-qt-tray";

QByteArray traySvg()
{
    QFile f(QStringLiteral(":/steamqt/icons/steam-qt-tray.svg"));
    return f.open(QIODevice::ReadOnly) ? f.readAll() : QByteArray();
}

// Install the symbolic icon into the user's hicolor theme so the tray host can load it by
// name. Plasma then recolours its ColorScheme-Text class to match the panel, whichever
// colour scheme (light, dark, or a dark panel with light windows) is active.
bool installThemeIcon(const QByteArray& svg)
{
    const QString dir = QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation)
        + QStringLiteral("/icons/hicolor/scalable/status");
    const QString path = dir + QLatin1Char('/') + QLatin1String(kTrayIconName) + QStringLiteral(".svg");
    QFile existing(path);
    if (existing.open(QIODevice::ReadOnly) && existing.readAll() == svg) {
        return true;
    }
    existing.close();
    QDir().mkpath(dir);
    QFile out(path);
    return out.open(QIODevice::WriteOnly | QIODevice::Truncate) && out.write(svg) == svg.size();
}

// Fallback for hosts that don't recolour symbolic icons: paint the logo in the palette's text
// colour, rebuilt whenever the palette changes (light/dark switch).
QIcon recolouredIcon(const QByteArray& svg, const QColor& colour)
{
    QByteArray data = svg;
    data.replace("#232629", colour.name().toLatin1());
    QIcon icon;
    for (int size : {16, 22, 24, 32, 48, 64}) {
        QPixmap pm;
        if (pm.loadFromData(data, "SVG")) {
            icon.addPixmap(pm.scaled(size, size, Qt::KeepAspectRatio, Qt::SmoothTransformation));
        }
    }
    return icon;
}

class Tray : public QObject
{
public:
    Tray(QQuickWindow* window, QObject* library)
        : m_window(window)
        , m_library(library)
        , m_svg(traySvg())
    {
        installThemeIcon(m_svg);
        m_tray.setToolTip(QStringLiteral("Steam-QT"));
        updateIcon();

        auto* toggle = m_menu.addAction(QString());
        QObject::connect(toggle, &QAction::triggered, this, [this] { toggleWindow(); });
        QObject::connect(&m_menu, &QMenu::aboutToShow, this, [this, toggle] {
            toggle->setText(m_window->isVisible() ? QObject::tr("Hide Steam-QT") : QObject::tr("Show Steam-QT"));
        });
        m_menu.addSeparator();
        QObject::connect(m_menu.addAction(QIcon::fromTheme(QStringLiteral("application-exit")), QObject::tr("Quit")),
                         &QAction::triggered, qApp, &QCoreApplication::quit);
        m_tray.setContextMenu(&m_menu);

        QObject::connect(&m_tray, &QSystemTrayIcon::activated, this, [this](QSystemTrayIcon::ActivationReason reason) {
            if (reason == QSystemTrayIcon::Trigger || reason == QSystemTrayIcon::DoubleClick) {
                toggleWindow();
            }
        });
        if (m_library) {
            // Download status in the tooltip. Tray isn't a moc'd class, so the QML property's
            // notify signal is routed through a single-shot timer's start() slot.
            m_tooltipTimer.setSingleShot(true);
            m_tooltipTimer.setInterval(0);
            QObject::connect(&m_tooltipTimer, &QTimer::timeout, this, [this] { updateTooltip(); });
            const QMetaObject* mo = m_library->metaObject();
            const int signal = mo->indexOfSignal("footerChanged()");
            const int slot = m_tooltipTimer.metaObject()->indexOfSlot("start()");
            if (signal >= 0 && slot >= 0) {
                QObject::connect(m_library, mo->method(signal), &m_tooltipTimer, m_tooltipTimer.metaObject()->method(slot));
            }
            updateTooltip();
        }
        qApp->installEventFilter(this);
        m_tray.show();
    }

    void updateTooltip()
    {
        const QString footer = m_library ? m_library->property("footer").toString() : QString();
        m_tray.setToolTip(footer.isEmpty() ? QStringLiteral("Steam-QT") : QStringLiteral("Steam-QT\n") + footer);
    }

protected:
    bool eventFilter(QObject* watched, QEvent* event) override
    {
        if (watched == qApp && event->type() == QEvent::ApplicationPaletteChange) {
            updateIcon();
        }
        return QObject::eventFilter(watched, event);
    }

private:
    void updateIcon()
    {
        const QIcon fallback = recolouredIcon(m_svg, qApp->palette().color(QPalette::WindowText));
        // Prefer the named theme icon (recoloured by the tray host); fall back to our own.
        m_tray.setIcon(QIcon::fromTheme(QLatin1String(kTrayIconName), fallback));
    }

    void toggleWindow()
    {
        // Toggle on visibility, not focus: clicking the tray gives the panel focus first, so the
        // window is never "active" at that moment.
        if (m_window->isVisible() && m_window->windowState() != Qt::WindowMinimized) {
            m_window->hide();
        } else {
            m_window->showNormal();
            m_window->raise();
            m_window->requestActivate();
        }
    }

    QQuickWindow* m_window;
    QObject* m_library;
    QByteArray m_svg;
    QSystemTrayIcon m_tray;
    QMenu m_menu;
    QTimer m_tooltipTimer;
};

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

    // System tray: closing the window keeps Steam-QT running (downloads continue), like Steam.
    std::unique_ptr<Tray> tray;
    auto* window = engine.rootObjects().isEmpty() ? nullptr : qobject_cast<QQuickWindow*>(engine.rootObjects().first());
    if (window && QSystemTrayIcon::isSystemTrayAvailable() && qgetenv("STEAMQT_NO_TRAY").isEmpty()) {
        tray = std::make_unique<Tray>(window, window->findChild<QObject*>(QStringLiteral("library")));
        QApplication::setQuitOnLastWindowClosed(false);
    }
    return app.exec();
}
