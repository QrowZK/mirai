//! Persistent user profile: bookmarks and settings, stored as JSON in the
//! platform data directory. Everything stays local — nothing syncs anywhere.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub homepage: String,
    /// Search URL template; `{}` is replaced with the URL-encoded query.
    pub search_template: String,
    pub blocking_enabled: bool,
    /// Download directory; `None` means the platform Downloads folder.
    pub downloads_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            homepage: crate::DEFAULT_URL.to_owned(),
            search_template: "https://duckduckgo.com/?q={}".to_owned(),
            blocking_enabled: true,
            downloads_dir: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Profile {
    pub bookmarks: Vec<Bookmark>,
    pub settings: Settings,
}

impl Profile {
    pub fn load() -> Profile {
        let Some(path) = profile_path() else {
            return Profile::default();
        };
        std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = profile_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_vec_pretty(self) {
            Ok(bytes) => {
                if let Err(error) = std::fs::write(&path, bytes) {
                    log::warn!("could not save profile: {error}");
                }
            }
            Err(error) => log::warn!("could not serialize profile: {error}"),
        }
    }

    pub fn is_bookmarked(&self, url: &str) -> bool {
        self.bookmarks.iter().any(|bookmark| bookmark.url == url)
    }

    /// Add or remove a bookmark for `url`. Returns true if it is now bookmarked.
    pub fn toggle_bookmark(&mut self, title: String, url: String) -> bool {
        if let Some(position) = self
            .bookmarks
            .iter()
            .position(|bookmark| bookmark.url == url)
        {
            self.bookmarks.remove(position);
            false
        } else {
            let title = if title.is_empty() { url.clone() } else { title };
            self.bookmarks.push(Bookmark { title, url });
            true
        }
    }

    pub fn downloads_dir(&self) -> PathBuf {
        if let Some(dir) = &self.settings.downloads_dir {
            return dir.clone();
        }
        default_downloads_dir()
    }
}

/// The platform data directory for Mirai (profile, bookmarks).
pub fn data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    Some(base?.join("mirai"))
}

fn profile_path() -> Option<PathBuf> {
    Some(data_dir()?.join("profile.json"))
}

fn default_downloads_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("USERPROFILE").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("HOME").map(PathBuf::from);
    base.map(|home| home.join("Downloads"))
        .unwrap_or_else(std::env::temp_dir)
}
