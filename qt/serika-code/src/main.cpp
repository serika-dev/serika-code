#include "ipcclient.h"

#include <QCommandLineParser>
#include <QGuiApplication>
#include <QIcon>
#include <QPalette>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QTimer>

int main(int argc, char *argv[]) {
    QGuiApplication app(argc, argv);
    QGuiApplication::setApplicationName(QStringLiteral("Serika Code"));
    QGuiApplication::setOrganizationName(QStringLiteral("Serika"));
    // Linux desktops match the window to serika-code.desktop (its icon, name and dock pin).
    QGuiApplication::setDesktopFileName(QStringLiteral("serika-code"));
    QIcon icon;
    for (int size : {16, 32, 48, 64, 128, 256})
        icon.addFile(QStringLiteral(":/qt/qml/SerikaCode/app/serika-code-%1.png").arg(size), QSize(size, size));
    QGuiApplication::setWindowIcon(icon);
    QQuickStyle::setStyle(QStringLiteral("Basic"));

    QPalette palette = QGuiApplication::palette();
    palette.setColor(QPalette::Link, QColor(QStringLiteral("#a78bfa")));
    palette.setColor(QPalette::LinkVisited, QColor(QStringLiteral("#a78bfa")));
    QGuiApplication::setPalette(palette);

    QCommandLineParser parser;
    parser.addHelpOption();
    parser.addOption({QStringLiteral("demo"), QStringLiteral("Use sample data instead of the signed-in account.")});
    parser.addOption({QStringLiteral("shot"), QStringLiteral("Save a screenshot to <png> and exit."), QStringLiteral("png")});
    parser.addOption({QStringLiteral("page"), QStringLiteral("Start on login, welcome, or app."), QStringLiteral("page")});
    parser.addOption({QStringLiteral("popup"), QStringLiteral("Open models, account, chat, settings, account-settings, shortcuts, palette, or project."), QStringLiteral("popup")});
    parser.addOption({QStringLiteral("theme"), QStringLiteral("Force dark or light."), QStringLiteral("theme")});
    parser.addOption({QStringLiteral("preset"), QStringLiteral("Theme preset id, for example nord."), QStringLiteral("preset")});
    parser.addOption({QStringLiteral("tab"), QStringLiteral("Settings section to open with --popup settings."), QStringLiteral("tab")});
    parser.addOption({QStringLiteral("panel"), QStringLiteral("Open the side panel on diffs or browser."), QStringLiteral("panel")});
    parser.process(app);

    const bool demo = parser.isSet(QStringLiteral("demo"));
    IpcClient ipc(demo);
    QQmlApplicationEngine engine;
    engine.rootContext()->setContextProperty(QStringLiteral("ipc"), &ipc);
    engine.rootContext()->setContextProperty(QStringLiteral("startPage"), parser.value(QStringLiteral("page")));
    engine.rootContext()->setContextProperty(QStringLiteral("startPopup"), parser.value(QStringLiteral("popup")));
    engine.rootContext()->setContextProperty(QStringLiteral("forcedTheme"), parser.value(QStringLiteral("theme")));
    engine.rootContext()->setContextProperty(QStringLiteral("startPreset"), parser.value(QStringLiteral("preset")));
    engine.rootContext()->setContextProperty(QStringLiteral("startTab"), parser.value(QStringLiteral("tab")));
    engine.rootContext()->setContextProperty(QStringLiteral("startPanel"), parser.value(QStringLiteral("panel")));
    engine.loadFromModule("SerikaCode", "Main");
    if (engine.rootObjects().isEmpty())
        return 1;

    if (parser.isSet(QStringLiteral("shot"))) {
        const QString path = parser.value(QStringLiteral("shot"));
        QTimer::singleShot(4500, &app, [&engine, &app, path]() {
            auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().constFirst());
            if (window)
                window->grabWindow().save(path);
            app.quit();
        });
    }
    return app.exec();
}
