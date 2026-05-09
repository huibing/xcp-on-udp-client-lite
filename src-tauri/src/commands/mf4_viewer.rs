use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mf4_parse::{DataValue, Mf4Wrapper};
use serde::Serialize;
use serde_json::json;
use tauri::ipc::Response;
use tauri::State;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Mf4RecordFile {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
}

pub struct Mf4FileCache {
    inner: Arc<Mutex<HashMap<String, Mf4Wrapper>>>,
}

impl Mf4FileCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_channel_names(&self, path: &str) -> Option<Vec<String>> {
        let cache = self.inner.lock().unwrap();
        cache.get(path).map(|mf4| mf4.get_channel_names())
    }

    fn insert(&self, path: String, mf4: Mf4Wrapper) {
        self.inner.lock().unwrap().insert(path, mf4);
    }
}

pub(crate) fn sort_channel_names(names: &mut [String]) {
    names.sort_by_cached_key(|name| name.to_ascii_lowercase());
}

pub(crate) fn resolve_records_dir(requested_dir: &Path, cwd: &Path) -> PathBuf {
    if requested_dir.is_absolute() {
        return requested_dir.to_path_buf();
    }

    let direct = cwd.join(requested_dir);
    if direct.exists() {
        return direct;
    }

    let src_tauri = cwd.join("src-tauri").join(requested_dir);
    if src_tauri.exists() {
        return src_tauri;
    }

    direct
}

pub(crate) fn collect_mf4_files(dir: &Path) -> Result<Vec<Mf4RecordFile>, String> {
    let mut files = std::fs::read_dir(dir)
        .map_err(|error| format!("Failed to read MF4 directory '{}': {}", dir.display(), error))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let ext = path.extension()?.to_str()?;
            if !ext.eq_ignore_ascii_case("mf4") {
                return None;
            }
            let meta = entry.metadata().ok()?;
            let name = path.file_name()?.to_string_lossy().to_string();
            Some(Mf4RecordFile {
                name,
                path: path.to_string_lossy().to_string(),
                size_bytes: meta.len(),
            })
        })
        .collect::<Vec<_>>();

    files.sort_by_cached_key(|file| file.name.to_ascii_lowercase());
    Ok(files)
}

pub(crate) fn f64_values_to_le_bytes(values: &[f64]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>()
}

#[tauri::command]
pub async fn mf4_list_record_files(directory: Option<String>) -> Result<serde_json::Value, String> {
    let cwd = std::env::current_dir().map_err(|error| format!("Failed to resolve cwd: {}", error))?;
    let requested = PathBuf::from(directory.unwrap_or_else(|| "records".to_string()));
    let resolved = resolve_records_dir(&requested, &cwd);

    if !resolved.exists() {
        return Ok(json!({
            "success": true,
            "exists": false,
            "directory": resolved.to_string_lossy().to_string(),
            "files": [],
        }));
    }

    let files = collect_mf4_files(&resolved)?;
    Ok(json!({
        "success": true,
        "exists": true,
        "directory": resolved.to_string_lossy().to_string(),
        "files": files,
    }))
}

#[tauri::command]
pub async fn mf4_open_file(
    mf4_path: String,
    cache: State<'_, Mf4FileCache>,
) -> Result<serde_json::Value, String> {
    let path = PathBuf::from(&mf4_path);
    let metadata = std::fs::metadata(&path)
        .map_err(|error| format!("Failed to stat MF4 file '{}': {}", path.display(), error))?;

    let mut channels = if let Some(cached) = cache.get_channel_names(&mf4_path) {
        cached
    } else {
        let load_path = path.clone();
        let parsed = tokio::task::spawn_blocking(move || {
            Mf4Wrapper::new(load_path, Option::<&fn(f64)>::None)
                .map_err(|error| error.to_string())
        })
            .await
            .map_err(|error| format!("MF4 load task failed: {}", error))?
            .map_err(|error| format!("Failed to open MF4 file: {}", error))?;
        let mut parsed_channels = parsed.get_channel_names();
        sort_channel_names(&mut parsed_channels);
        cache.insert(mf4_path.clone(), parsed);
        parsed_channels
    };
    sort_channel_names(&mut channels);

    Ok(json!({
        "success": true,
        "file": {
            "name": path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_else(|| mf4_path.clone()),
            "path": mf4_path,
            "size_bytes": metadata.len(),
        },
        "channel_count": channels.len(),
        "channels": channels,
    }))
}

#[tauri::command]
pub fn mf4_get_channel_data(
    mf4_path: String,
    channel_name: String,
    cache: State<'_, Mf4FileCache>,
) -> Result<Response, String> {
    let cache = cache.inner.lock().unwrap();
    let mf4 = cache
        .get(&mf4_path)
        .ok_or_else(|| format!("MF4 file '{}' is not loaded", mf4_path))?;
    let data = mf4
        .get_channel_data(&channel_name)
        .ok_or_else(|| format!("Channel '{}' was not found", channel_name))?;
    if !data.is_num() {
        return Err(format!("Channel '{}' is not numeric", channel_name));
    }
    let values: Vec<f64> = data
        .try_into()
        .map_err(|error| format!("Failed to convert channel '{}': {}", channel_name, error))?;
    Ok(Response::new(f64_values_to_le_bytes(&values)))
}

#[tauri::command]
pub fn mf4_get_channel_time(
    mf4_path: String,
    channel_name: String,
    cache: State<'_, Mf4FileCache>,
) -> Result<Response, String> {
    let cache = cache.inner.lock().unwrap();
    let mf4 = cache
        .get(&mf4_path)
        .ok_or_else(|| format!("MF4 file '{}' is not loaded", mf4_path))?;
    let time = mf4
        .get_channel_master_data(&channel_name)
        .ok_or_else(|| format!("Time base for '{}' was not found", channel_name))?;
    let values = match time {
        DataValue::REAL(values) => values,
        _ => {
            return Err(format!(
                "Time base for '{}' is not REAL/f64 data",
                channel_name
            ))
        }
    };
    Ok(Response::new(f64_values_to_le_bytes(&values)))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use uuid::Uuid;

    use super::{
        collect_mf4_files, f64_values_to_le_bytes, resolve_records_dir, sort_channel_names,
    };

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("mf4-viewer-test-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn sort_channel_names_orders_case_insensitively() {
        let mut names = vec![
            "zeta_speed".to_string(),
            "AlphaTemp".to_string(),
            "beta_rpm".to_string(),
        ];

        sort_channel_names(&mut names);

        assert_eq!(names, vec!["AlphaTemp", "beta_rpm", "zeta_speed"]);
    }

    #[test]
    fn resolve_records_dir_falls_back_to_src_tauri_records_for_relative_paths() {
        let temp = TempDir::new();
        let fallback = temp.path().join("src-tauri").join("records");
        fs::create_dir_all(&fallback).unwrap();

        let resolved = resolve_records_dir(Path::new("records"), temp.path());

        assert_eq!(resolved, fallback);
    }

    #[test]
    fn collect_mf4_files_filters_non_mf4_entries() {
        let temp = TempDir::new();
        fs::write(temp.path().join("run_b.MF4"), b"mf4").unwrap();
        fs::write(temp.path().join("run_a.mf4"), b"mf4").unwrap();
        fs::write(temp.path().join("notes.txt"), b"txt").unwrap();

        let files = collect_mf4_files(temp.path()).unwrap();

        let names = files.iter().map(|file| file.name.clone()).collect::<Vec<_>>();
        assert_eq!(names, vec!["run_a.mf4", "run_b.MF4"]);
    }

    #[test]
    fn f64_values_to_le_bytes_round_trips() {
        let values = [1.25_f64, -2.5_f64, 42.0_f64];

        let bytes = f64_values_to_le_bytes(&values);

        assert_eq!(bytes.len(), values.len() * std::mem::size_of::<f64>());

        let decoded = bytes
            .chunks_exact(std::mem::size_of::<f64>())
            .map(|chunk| {
                let mut arr = [0_u8; 8];
                arr.copy_from_slice(chunk);
                f64::from_le_bytes(arr)
            })
            .collect::<Vec<_>>();
        assert_eq!(decoded, values);
    }
}
