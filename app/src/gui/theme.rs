//! Follows the Omarchy theme live: the palette from colors.toml, the
//! wallpaper, Hyprland's window rounding, and the interface font. A theme
//! switch replaces files and symlinks, so instead of watching paths the
//! window polls the theme's generation counter, which every switch bumps.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, accent)]
        #[qproperty(QString, background)]
        #[qproperty(QString, surface)]
        #[qproperty(QString, raised)]
        #[qproperty(QString, foreground)]
        #[qproperty(QString, bright)]
        #[qproperty(QString, muted)]
        #[qproperty(QString, selection)]
        #[qproperty(QString, red)]
        #[qproperty(QString, green)]
        #[qproperty(QString, yellow)]
        #[qproperty(bool, dark)]
        #[qproperty(i32, radius)]
        #[qproperty(QString, font)]
        #[qproperty(QString, wallpaper)]
        type Theme = super::ThemeRust;

        /// Re-reads the theme if it changed since the last look.
        #[qinvokable]
        fn poll(self: Pin<&mut Theme>);
    }

    impl cxx_qt::Initialize for Theme {}
}

use std::path::PathBuf;
use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;

pub struct ThemeRust {
    accent: QString,
    background: QString,
    surface: QString,
    raised: QString,
    foreground: QString,
    bright: QString,
    muted: QString,
    selection: QString,
    red: QString,
    green: QString,
    yellow: QString,
    dark: bool,
    radius: i32,
    font: QString,
    wallpaper: QString,
    generation: String,
}

impl Default for ThemeRust {
    /// Omarchy's fallback accent on a neutral dark palette, for when Omarchy
    /// isn't there.
    fn default() -> Self {
        Self {
            accent: QString::from("#FFD60A"),
            background: QString::from("#121212"),
            surface: QString::from("#0c0c0c"),
            raised: QString::from("#1e1e1e"),
            foreground: QString::from("#d4d4d4"),
            bright: QString::from("#f5f5f5"),
            muted: QString::from("#7a7a7a"),
            selection: QString::from("#2a2a2a"),
            red: QString::from("#f7768e"),
            green: QString::from("#9ece6a"),
            yellow: QString::from("#e0af68"),
            dark: true,
            radius: 0,
            font: QString::from("sans-serif"),
            wallpaper: QString::default(),
            generation: String::from("unread"),
        }
    }
}

fn theme_dir() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        });
    state.join("omarchy/current")
}

/// Reads `key = "value"` pairs from colors.toml. The file is flat, so a full
/// TOML parse buys nothing.
fn colors(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            let v = v.trim().trim_matches('"');
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

fn hyprland_rounding() -> i32 {
    std::process::Command::new("hyprctl")
        .args(["getoption", "decoration:rounding", "-j"])
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| v.get("int").and_then(|i| i.as_i64()))
        .map(|r| r.clamp(0, 24) as i32)
        .unwrap_or(0)
}

fn omarchy_font() -> Option<String> {
    let out = std::process::Command::new("omarchy-font-current")
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

impl cxx_qt::Initialize for qobject::Theme {
    fn initialize(self: Pin<&mut Self>) {
        self.poll();
    }
}

impl qobject::Theme {
    fn poll(mut self: Pin<&mut Self>) {
        let dir = theme_dir();
        let generation = std::fs::read_to_string(dir.join("theme.generation"))
            .unwrap_or_default()
            .trim()
            .to_string();
        if generation == self.generation && self.generation != "unread" {
            return;
        }
        self.as_mut().rust_mut().generation = generation;

        let Ok(text) = std::fs::read_to_string(dir.join("theme/colors.toml")) else {
            return;
        };
        let c = colors(&text);
        let get = |k: &str, fallback: &QString| {
            c.get(k)
                .map(|v| QString::from(v.as_str()))
                .unwrap_or_else(|| fallback.clone())
        };
        let d = ThemeRust::default();
        let accent = get("accent", &d.accent);
        let background = get("background", &d.background);
        let surface = get("dark_background", &background);
        let raised = get("lighter_background", &d.raised);
        let foreground = get("foreground", &d.foreground);
        let bright = get("bright_foreground", &foreground);
        let muted = get("dark_foreground", &d.muted);
        let selection = get("selection", &d.selection);
        let red = get("red", &d.red);
        let green = get("green", &d.green);
        let yellow = get("yellow", &d.yellow);
        self.as_mut().set_accent(accent);
        self.as_mut().set_background(background);
        self.as_mut().set_surface(surface);
        self.as_mut().set_raised(raised);
        self.as_mut().set_foreground(foreground);
        self.as_mut().set_bright(bright);
        self.as_mut().set_muted(muted);
        self.as_mut().set_selection(selection);
        self.as_mut().set_red(red);
        self.as_mut().set_green(green);
        self.as_mut().set_yellow(yellow);
        self.as_mut()
            .set_dark(c.get("mode").map(|m| m != "light").unwrap_or(true));
        self.as_mut().set_radius(hyprland_rounding());
        if let Some(f) = omarchy_font() {
            self.as_mut().set_font(QString::from(f.as_str()));
        }
        let wallpaper = std::fs::canonicalize(dir.join("background"))
            .map(|p| format!("file://{}", p.display()))
            .unwrap_or_default();
        self.as_mut()
            .set_wallpaper(QString::from(wallpaper.as_str()));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_flat_colors() {
        let c = super::colors("mode = \"dark\"\n\naccent = \"#7aa2f7\"\n# note\n");
        assert_eq!(c["accent"], "#7aa2f7");
        assert_eq!(c["mode"], "dark");
    }
}
