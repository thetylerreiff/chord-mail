use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("chord").qml_files([
        "qml/Main.qml",
        "qml/ThreadList.qml",
        "qml/ThreadRow.qml",
        "qml/ThreadPage.qml",
        "qml/ReviewPage.qml",
        "qml/Picker.qml",
        "qml/SearchBar.qml",
        "qml/HelpOverlay.qml",
        "qml/Toast.qml",
        "qml/StatusBar.qml",
        "qml/SetupPage.qml",
    ]))
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .qt_module("WebEngineQuick")
    .qt_module("Gui")
    .qt_module("Test")
    .files(["src/gui/bridge.rs", "src/gui/theme.rs"])
    .cpp_file("cpp/init.cpp")
    .build();
}
