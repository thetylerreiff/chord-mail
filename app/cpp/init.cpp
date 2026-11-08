// Qt calls cxx-qt-lib doesn't bind, kept to the minimum.
#include <QtGui/QGuiApplication>
#include <QtGui/QImage>
#include <QtQuick/QQuickWindow>
#include <QtWebEngineQuick/qtwebenginequickglobal.h>

// Qt WebEngine must be initialized before the application object exists.
extern "C" void chord_init_webengine() { QtWebEngineQuick::initialize(); }

// Saves the window, popups and web views included, for the screenshot tour.
extern "C" bool chord_grab_window(const char *path) {
    for (QWindow *w : QGuiApplication::topLevelWindows()) {
        if (auto *quick = qobject_cast<QQuickWindow *>(w)) {
            return quick->grabWindow().save(QString::fromUtf8(path));
        }
    }
    return false;
}

#include <QtGui/QKeySequence>
// QTest only declares its QWindow overloads when it knows QtGui is linked.
#ifndef QT_GUI_LIB
#define QT_GUI_LIB
#endif
#include <QtTest/QTest>

// Presses a key the way a keyboard would, through the shortcut map, for the
// screenshot tour. `spec` is a QKeySequence string ("J", "Shift+E", "Ctrl+K")
// or a single symbol ("#", "?"), which is typed with Shift as on a US layout.
extern "C" void chord_press(const char *spec_utf8) {
    QWindow *window = QGuiApplication::focusWindow();
    if (!window && !QGuiApplication::topLevelWindows().isEmpty())
        window = QGuiApplication::topLevelWindows().first();
    if (!window)
        return;
    const QString spec = QString::fromUtf8(spec_utf8);
    if (spec.size() == 1 && !spec[0].isLetterOrNumber()) {
        const bool shifted = QStringLiteral("!@#$%^&*()_+{}|:\"<>?~").contains(spec[0]);
        QTest::keyClick(window, spec[0].toLatin1(), shifted ? Qt::ShiftModifier : Qt::NoModifier);
        return;
    }
    if (spec.size() == 1) {
        QTest::keyClick(window, spec[0].toLower().toLatin1());
        return;
    }
    const QKeyCombination combo = QKeySequence::fromString(spec)[0];
    QTest::keyClick(window, combo.key(), combo.keyboardModifiers());
}
