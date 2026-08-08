//! Chrome-level download handling.
//!
//! Servo's embedder API has no download support yet, so Mirai implements the
//! common case itself: a navigation that points at a file-type URL is denied
//! in the engine and fetched directly to the user's download directory, with
//! progress surfaced in the settings window.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use libservo::EventLoopWaker;
use url::Url;

/// File extensions treated as downloads when navigated to. Content served
/// with `Content-Disposition: attachment` on ordinary URLs is not detected;
/// that requires engine support in Servo.
const DOWNLOAD_EXTENSIONS: &[&str] = &[
    "7z", "apk", "appimage", "bin", "bz2", "deb", "dmg", "doc", "docx", "exe", "flac", "gz", "iso",
    "jar", "msi", "odp", "ods", "odt", "pdf", "pkg", "ppt", "pptx", "rar", "rpm", "tar", "torrent",
    "xls", "xlsx", "xz", "zip",
];

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadStatus {
    InProgress,
    Complete,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct DownloadEntry {
    pub file_name: String,
    pub path: PathBuf,
    pub received: u64,
    pub total: Option<u64>,
    pub status: DownloadStatus,
}

pub type DownloadList = Arc<Mutex<Vec<DownloadEntry>>>;

/// Whether a navigation to `url` should be treated as a file download.
pub fn is_download_url(url: &Url) -> bool {
    if url.scheme() != "http" && url.scheme() != "https" {
        return false;
    }
    let path = url.path().to_ascii_lowercase();
    DOWNLOAD_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(&format!(".{extension}")))
}

fn file_name_for(url: &Url) -> String {
    url.path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .unwrap_or("download")
        .to_owned()
}

/// Pick a path in `dir` that doesn't collide with an existing file.
fn unique_path(dir: &std::path::Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, extension) = match file_name.rsplit_once('.') {
        Some((stem, extension)) => (stem, format!(".{extension}")),
        None => (file_name, String::new()),
    };
    (1..)
        .map(|counter| dir.join(format!("{stem} ({counter}){extension}")))
        .find(|path| !path.exists())
        .expect("some counter is always free")
}

/// Start a download of `url` into `dir` on a background thread. Progress is
/// recorded in `list`; `waker` nudges the UI event loop on every update.
pub fn start_download(url: Url, dir: PathBuf, list: DownloadList, waker: Box<dyn EventLoopWaker>) {
    let file_name = file_name_for(&url);
    let index = {
        let mut downloads = list.lock().unwrap();
        downloads.push(DownloadEntry {
            file_name: file_name.clone(),
            path: dir.clone(),
            received: 0,
            total: None,
            status: DownloadStatus::InProgress,
        });
        downloads.len() - 1
    };

    std::thread::Builder::new()
        .name(format!("download {file_name}"))
        .spawn(move || {
            let finish = |list: &DownloadList, status: DownloadStatus| {
                list.lock().unwrap()[index].status = status;
                waker.wake();
            };
            let result = (|| -> Result<(), String> {
                std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
                let response = ureq::get(url.as_str())
                    .call()
                    .map_err(|error| error.to_string())?;
                let total = response
                    .header("Content-Length")
                    .and_then(|value| value.parse::<u64>().ok());
                let path = unique_path(&dir, &file_name);
                {
                    let mut downloads = list.lock().unwrap();
                    downloads[index].total = total;
                    downloads[index].path = path.clone();
                }

                let mut reader = response.into_reader();
                let mut file = std::fs::File::create(&path).map_err(|error| error.to_string())?;
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    let read = reader
                        .read(&mut buffer)
                        .map_err(|error| error.to_string())?;
                    if read == 0 {
                        break;
                    }
                    file.write_all(&buffer[..read])
                        .map_err(|error| error.to_string())?;
                    let mut downloads = list.lock().unwrap();
                    downloads[index].received += read as u64;
                    drop(downloads);
                    waker.wake();
                }
                Ok(())
            })();

            match result {
                Ok(()) => finish(&list, DownloadStatus::Complete),
                Err(error) => {
                    log::warn!("download of {url} failed: {error}");
                    finish(&list, DownloadStatus::Failed(error));
                }
            }
        })
        .expect("failed to spawn download thread");
}
