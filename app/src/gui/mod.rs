//! The window: a Qt Quick UI over the mail engine.

pub mod bridge;
pub mod theme;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
    // With no window, exec() would wait forever on nothing; Qt has already
    // printed the QML errors, so say what happened and leave.
    let failed = Arc::new(AtomicBool::new(engine.is_null()));
    if let Some(mut engine) = engine.as_mut() {
        let flag = failed.clone();
        // load() builds the window from compiled-in QML before it returns,
        // so the connection only needs to outlive it.
        let _connection = engine
            .as_mut()
            .on_object_creation_failed(move |_, _| flag.store(true, Ordering::Relaxed));
        engine.load(&QUrl::from("qrc:/qt/qml/chord/qml/Main.qml"));
    }
    if failed.load(Ordering::Relaxed) {
        eprintln!("chord: the window failed to load");
        instance::cleanup();
        return 1;
    }
    let code = match app.as_mut() {
        Some(app) => app.exec(),
        None => 1,
    };
    instance::cleanup();
    code
}
