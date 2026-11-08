//! The window: a Qt Quick UI over the mail engine.

pub mod bridge;
pub mod theme;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};

use crate::instance::{self, Request};

unsafe extern "C" {
    fn chord_init_webengine();
}

pub fn run(request: Request) -> i32 {
    let _ = bridge::INITIAL.set(std::sync::Mutex::new(Some(request)));
    // SAFETY: called once, before the application object exists, as Qt
    // WebEngine requires.
    unsafe { chord_init_webengine() };
    QQuickStyle::set_style(&QString::from("Material"));
    let mut app = QGuiApplication::new();
    // Matches the .desktop file, so Hyprland and the launcher pair the
    // window with its entry and icon.
    QGuiApplication::set_desktop_file_name(&QString::from("chord-mail"));
    if let Some(mut app) = app.as_mut() {
        app.as_mut().set_application_name(&QString::from("Chord"));
        app.as_mut()
            .set_application_version(&QString::from(env!("CARGO_PKG_VERSION")));
    }
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/chord/qml/Main.qml"));
    }
    let code = match app.as_mut() {
        Some(app) => app.exec(),
        None => 1,
    };
    instance::cleanup();
    code
}
