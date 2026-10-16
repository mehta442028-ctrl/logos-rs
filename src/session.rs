//! Where the reader left off: the passage, the panes that were open and the
//! texts in them, restored on the next launch.
//!
//! Everything here is a *hint*, never a constraint. Translations are keyed by
//! code and the reader's cut-off is stored as its value, so a saved session
//! stays meaningful when the database is rebuilt with a different set of texts
//! or the list of cut-offs changes. Anything that no longer resolves is simply
//! dropped and the default takes over — a stale session file must never stop
//! the app from starting.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// Translation codes rather than list indices: indices shift whenever the
    /// database gains or loses a text.
    pub primary: String,
    pub secondary: String,
    /// Additional visible translations (third and fourth columns).
    pub extra_translations: Vec<String>,
    /// Translation codes hidden from selection menus.
    pub hidden_translations: Vec<String>,
    pub book: i64,
    pub chapter: i64,
    pub parallel: bool,
    pub scroll_lock: bool,
    pub reader: bool,
    /// The cut-off the reader chose by hand, as a count. `None` leaves each
    /// corpus on its own conventional default.
    pub threshold: Option<i64>,
    pub dark: bool,
    /// Window size in *logical* pixels — physical ones would be re-scaled by
    /// the display factor on restore, so a HiDPI window would double each run,
    /// and the file would not survive moving between screens.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            primary: String::new(),
            secondary: String::new(),
            extra_translations: Vec::new(),
            hidden_translations: Vec::new(),
            book: 1,
            chapter: 1,
            parallel: false,
            scroll_lock: true,
            reader: false,
            threshold: None,
            dark: true,
            width: None,
            height: None,
        }
    }
}

/// `$LOGOS_SESSION`, else the platform config directory.
pub fn path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LOGOS_SESSION") {
        return Some(PathBuf::from(p));
    }
    directories::ProjectDirs::from("", "", "leverbe-parallel")
        .map(|d| d.config_dir().join("session.json"))
}

/// Load the last session, or the defaults if there is nothing readable. A
/// corrupt file is treated as absent rather than reported: the reader cannot
/// act on the error, and losing the last passage is not worth a failed start.
pub fn load() -> Session {
    let Some(path) = path() else { return Session::default() };
    let Ok(raw) = std::fs::read_to_string(&path) else { return Session::default() };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Write the session, creating the config directory if needed. Failures are
/// silent for the same reason.
pub fn save(session: &Session) {
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(raw) = serde_json::to_string_pretty(session) {
        let _ = std::fs::write(&path, raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_and_missing_fields_fall_back() {
        // A file written by an older or newer build must still load.
        let s: Session = serde_json::from_str(r#"{"book":40,"whatever":true}"#).unwrap();
        assert_eq!(s.book, 40);
        assert_eq!(s.chapter, 1); // defaulted
        assert!(s.scroll_lock); // defaulted, not false
        assert_eq!(s.threshold, None);
    }

    #[test]
    fn garbage_loads_as_default() {
        assert!(serde_json::from_str::<Session>("not json").is_err());
        let round = serde_json::to_string(&Session::default()).unwrap();
        let back: Session = serde_json::from_str(&round).unwrap();
        assert_eq!(back.chapter, 1);
    }
}
