//! Installed Komari-style theme bundles. Bundles are trusted administrator code.
use std::{
    fs::{self, File, OpenOptions},
    io::{Cursor, Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    sync::{Mutex, RwLock},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;
use zip::ZipArchive;

const MAX_ARCHIVE_BYTES: usize = 16 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;
const MAX_THEMES: usize = 32;
const MAX_SETTINGS_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ThemeManifest {
    pub name: Value,
    pub short: String,
    pub version: String,
    #[serde(default)]
    pub description: Value,
    #[serde(default)]
    pub author: Value,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub configuration: Value,
}

#[derive(Serialize)]
pub(crate) struct ThemeListing {
    pub active: String,
    pub themes: Vec<ThemeManifest>,
}

pub(crate) struct ThemeStore {
    root: PathBuf,
    active: RwLock<String>,
    install_lock: Mutex<()>,
}

fn valid_short(short: &str) -> bool {
    !short.is_empty()
        && short.len() <= 64
        && short
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !short.starts_with('.')
}

fn safe_file_path(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
}

fn private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|_| "cannot create theme directory".to_owned())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| "cannot secure theme directory".to_owned())
}

fn atomic_private_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("invalid theme path")?;
    let temporary = parent.join(format!(".write-{}", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| "cannot create theme settings")?;
        file.write_all(contents)
            .map_err(|_| "cannot write theme settings")?;
        file.sync_all().map_err(|_| "cannot sync theme settings")?;
        fs::rename(&temporary, path).map_err(|_| "cannot publish theme settings")?;
        File::open(parent)
            .and_then(|dir| dir.sync_all())
            .map_err(|_| "cannot sync theme directory")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

impl ThemeStore {
    pub(crate) fn open(database_path: &Path) -> Result<Self, String> {
        let parent = database_path.parent().ok_or("database has no parent")?;
        let root = parent.join("themes");
        private_dir(&root)?;
        let active = fs::read_to_string(root.join("active"))
            .unwrap_or_else(|_| "emerald".to_owned())
            .trim()
            .to_owned();
        let active = if active == "emerald" || (valid_short(&active) && root.join(&active).is_dir())
        {
            active
        } else {
            "emerald".to_owned()
        };
        Ok(Self {
            root,
            active: RwLock::new(active),
            install_lock: Mutex::new(()),
        })
    }

    pub(crate) fn active(&self) -> String {
        self.active
            .read()
            .map_or_else(|_| "emerald".to_owned(), |value| value.clone())
    }

    fn installed(&self, short: &str) -> bool {
        valid_short(short)
            && short != "emerald"
            && self.root.join(short).is_dir()
            && !fs::symlink_metadata(self.root.join(short))
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
    }

    fn manifest(&self, short: &str) -> Result<ThemeManifest, String> {
        if !self.installed(short) {
            return Err("theme not installed".to_owned());
        }
        let bytes = fs::read(self.root.join(short).join("komari-theme.json"))
            .map_err(|_| "theme manifest unavailable")?;
        if bytes.len() > MAX_SETTINGS_BYTES {
            return Err("theme manifest too large".to_owned());
        }
        let manifest: ThemeManifest =
            serde_json::from_slice(&bytes).map_err(|_| "invalid theme manifest")?;
        if manifest.short != short {
            return Err("theme manifest does not match directory".to_owned());
        }
        Ok(manifest)
    }

    pub(crate) fn list(&self) -> ThemeListing {
        let mut themes = vec![ThemeManifest {
            name: Value::String("Emerald".to_owned()),
            short: "emerald".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            description: Value::String("Pulse built-in theme".to_owned()),
            author: Value::String("Petauron".to_owned()),
            preview: String::new(),
            configuration: Value::Null,
        }];
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten().take(MAX_THEMES) {
                let short = entry.file_name().to_string_lossy().to_string();
                if let Ok(manifest) = self.manifest(&short) {
                    themes.push(manifest);
                }
            }
        }
        themes.sort_by(|a, b| a.short.cmp(&b.short));
        ThemeListing {
            active: self.active(),
            themes,
        }
    }

    // Keep validation, staging, publication and cleanup in one auditable transaction.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn install(&self, archive_bytes: &[u8]) -> Result<ThemeManifest, String> {
        let _guard = self
            .install_lock
            .lock()
            .map_err(|_| "theme installation lock unavailable")?;
        // Serialize install attempts so the package count and target checks stay coherent.
        if archive_bytes.is_empty() || archive_bytes.len() > MAX_ARCHIVE_BYTES {
            return Err("theme ZIP must be at most 16 MiB".to_owned());
        }
        if self.list().themes.len() >= MAX_THEMES {
            return Err("theme limit reached".to_owned());
        }
        let mut archive =
            ZipArchive::new(Cursor::new(archive_bytes)).map_err(|_| "invalid theme ZIP")?;
        if archive.is_empty() || archive.len() > MAX_ENTRIES {
            return Err("invalid theme ZIP entry count".to_owned());
        }
        let manifest_bytes = {
            let entry = archive
                .by_name("komari-theme.json")
                .map_err(|_| "missing root komari-theme.json")?;
            if entry.size() > MAX_SETTINGS_BYTES as u64 {
                return Err("theme manifest too large".to_owned());
            }
            let mut bytes = Vec::new();
            entry
                .take(MAX_SETTINGS_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "cannot read theme manifest")?;
            bytes
        };
        if manifest_bytes.len() > MAX_SETTINGS_BYTES {
            return Err("theme manifest too large".to_owned());
        }
        let manifest: ThemeManifest =
            serde_json::from_slice(&manifest_bytes).map_err(|_| "invalid theme manifest")?;
        if !valid_short(&manifest.short) || manifest.short == "emerald" {
            return Err("invalid or reserved theme short name".to_owned());
        }
        if manifest.version.trim().is_empty() || manifest.version.len() > 64 {
            return Err("invalid theme version".to_owned());
        }
        if manifest.preview.len() > 256
            || (!manifest.preview.is_empty() && !safe_file_path(Path::new(&manifest.preview)))
        {
            return Err("invalid theme preview path".to_owned());
        }
        if self
            .list()
            .themes
            .iter()
            .any(|theme| theme.short.eq_ignore_ascii_case(&manifest.short))
        {
            return Err("theme name already installed".to_owned());
        }

        let stage = self.root.join(format!(".install-{}", Uuid::new_v4()));
        private_dir(&stage)?;
        let result = (|| {
            let mut unpacked = 0u64;
            let mut has_index = false;
            let mut seen = std::collections::HashSet::new();
            for index in 0..archive.len() {
                let entry = archive
                    .by_index(index)
                    .map_err(|_| "cannot read ZIP entry")?;
                let name = entry.name();
                let path = Path::new(name);
                if !safe_file_path(path) || name.contains('\\') || name.contains('\0') {
                    return Err("unsafe ZIP path".to_owned());
                }
                if !seen.insert(name.to_ascii_lowercase()) {
                    return Err("duplicate ZIP path".to_owned());
                }
                let mode = entry.unix_mode().unwrap_or(0o100_644) & 0o170_000;
                if mode != 0 && mode != 0o100_000 && mode != 0o040_000 {
                    return Err("theme ZIP may not contain links or special files".to_owned());
                }
                if name == "komari-theme.json" {
                    continue;
                }
                let preview = name == manifest.preview
                    && matches!(
                        path.extension().and_then(|ext| ext.to_str()),
                        Some("png" | "jpg" | "jpeg" | "webp")
                    );
                if name != "dist" && !name.starts_with("dist/") && !preview {
                    return Err("theme ZIP must contain only manifest, preview and dist".to_owned());
                }
                if preview && entry.is_dir() {
                    return Err("theme preview must be a file".to_owned());
                }
                let target = stage.join(path);
                if entry.is_dir() {
                    private_dir(&target)?;
                    continue;
                }
                if entry.size() > MAX_FILE_BYTES
                    || unpacked.saturating_add(entry.size()) > MAX_UNPACKED_BYTES
                {
                    return Err("theme ZIP expands beyond limit".to_owned());
                }
                if name == "dist/index.html" {
                    has_index = true;
                }
                let parent = target.parent().ok_or("invalid theme path")?;
                private_dir(parent)?;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&target)
                    .map_err(|_| "duplicate or invalid theme file")?;
                let written = std::io::copy(&mut entry.take(MAX_FILE_BYTES + 1), &mut output)
                    .map_err(|_| "cannot unpack theme file")?;
                if written > MAX_FILE_BYTES {
                    return Err("theme file exceeds limit".to_owned());
                }
                unpacked = unpacked.saturating_add(written);
                if unpacked > MAX_UNPACKED_BYTES {
                    return Err("theme ZIP expands beyond limit".to_owned());
                }
                output.sync_all().map_err(|_| "cannot sync theme file")?;
            }
            if !has_index {
                return Err("missing dist/index.html".to_owned());
            }
            atomic_private_file(&stage.join("komari-theme.json"), &manifest_bytes)?;
            fs::rename(&stage, self.root.join(&manifest.short))
                .map_err(|_| "cannot publish theme")?;
            File::open(&self.root)
                .and_then(|dir| dir.sync_all())
                .map_err(|_| "cannot sync theme directory")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result.map(|()| manifest)
    }

    pub(crate) fn activate(&self, short: &str) -> Result<(), String> {
        if short != "emerald" && !self.installed(short) {
            return Err("theme not installed".to_owned());
        }
        atomic_private_file(&self.root.join("active"), short.as_bytes())?;
        short.clone_into(&mut *self.active.write().map_err(|_| "theme lock unavailable")?);
        Ok(())
    }

    pub(crate) fn settings(&self) -> Value {
        let short = self.active();
        if short == "emerald" {
            return Value::Null;
        }
        let Ok(manifest) = self.manifest(&short) else {
            return Value::Null;
        };
        let mut settings = Map::new();
        if manifest
            .configuration
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("managed")
            == "managed"
            && let Some(fields) = manifest.configuration.get("data").and_then(Value::as_array)
        {
            for field in fields.iter().take(64) {
                if let (Some(key), Some(default)) = (
                    field.get("key").and_then(Value::as_str),
                    field.get("default"),
                ) && !key.is_empty()
                    && key.len() <= 64
                {
                    settings.insert(key.to_owned(), default.clone());
                }
            }
        }
        if let Ok(bytes) = fs::read(self.root.join(short).join("settings.json"))
            && bytes.len() <= MAX_SETTINGS_BYTES
            && let Ok(Value::Object(saved)) = serde_json::from_slice::<Value>(&bytes)
        {
            settings.extend(saved);
        }
        Value::Object(settings)
    }

    pub(crate) fn save_settings(&self, short: &str, settings: &Value) -> Result<(), String> {
        if !self.installed(short) || !settings.is_object() {
            return Err("invalid theme settings".to_owned());
        }
        let bytes = serde_json::to_vec(settings).map_err(|_| "invalid theme settings")?;
        if bytes.len() > MAX_SETTINGS_BYTES {
            return Err("theme settings too large".to_owned());
        }
        atomic_private_file(&self.root.join(short).join("settings.json"), &bytes)
    }

    pub(crate) fn read_asset(&self, request_path: &str) -> Option<(String, Vec<u8>)> {
        let short = self.active();
        if short == "emerald" || !self.installed(&short) {
            return None;
        }
        let relative = if request_path.is_empty()
            || request_path == "index.html"
            || request_path
                .rsplit('/')
                .next()
                .is_some_and(|segment| !segment.contains('.'))
        {
            PathBuf::from("index.html")
        } else {
            PathBuf::from(request_path)
        };
        if !safe_file_path(&relative) {
            return None;
        }
        let root = self.root.join(short).join("dist");
        let target = root.join(&relative);
        let canonical_root = root.canonicalize().ok()?;
        let canonical_target = target.canonicalize().ok()?;
        if !canonical_target.starts_with(canonical_root) || !canonical_target.is_file() {
            return None;
        }
        let metadata = canonical_target.metadata().ok()?;
        if metadata.len() > MAX_FILE_BYTES {
            return None;
        }
        fs::read(canonical_target)
            .ok()
            .map(|bytes| (relative.to_string_lossy().into_owned(), bytes))
    }
}

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    routing::{get, post},
};
use serde_json::json;

use crate::{ApiError, AppState};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/theme/list", get(list_themes))
        .route("/api/admin/theme/set", post(set_theme))
        .route("/api/admin/theme/settings", post(save_theme_settings))
        .route(
            "/api/admin/theme/install",
            post(install_theme).layer(DefaultBodyLimit::max(MAX_ARCHIVE_BYTES)),
        )
}

async fn list_themes(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"status":"success","message":"","data":state.themes.list()}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetTheme {
    theme: String,
}

async fn set_theme(
    State(state): State<AppState>,
    Json(input): Json<SetTheme>,
) -> Result<Json<Value>, ApiError> {
    state
        .themes
        .activate(&input.theme)
        .map_err(ApiError::bad_request)?;
    Ok(Json(
        json!({"status":"success","message":"","data":{"theme":input.theme}}),
    ))
}

#[derive(Deserialize)]
struct ThemeQuery {
    theme: String,
}

async fn save_theme_settings(
    State(state): State<AppState>,
    Query(query): Query<ThemeQuery>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    state
        .themes
        .save_settings(&query.theme, &value)
        .map_err(ApiError::bad_request)?;
    Ok(Json(
        json!({"status":"success","message":"","data":{"theme":query.theme}}),
    ))
}

async fn install_theme(
    State(state): State<AppState>,
    archive: Bytes,
) -> Result<Json<Value>, ApiError> {
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err(ApiError::bad_request("theme ZIP exceeds 16 MiB"));
    }
    let store = state.themes.clone();
    let manifest = tokio::task::spawn_blocking(move || store.install(&archive))
        .await
        .map_err(|_| ApiError::unavailable("theme installation failed"))?
        .map_err(ApiError::bad_request)?;
    Ok(Json(
        json!({"status":"success","message":"","data":manifest}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_and_paths_are_bounded() {
        assert!(valid_short("hello-world_2"));
        assert!(!valid_short("../evil"));
        assert!(!valid_short(""));
        assert!(!safe_file_path(Path::new("../escape")));
        assert!(!safe_file_path(Path::new("/absolute")));
    }

    fn theme_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use zip::{ZipWriter, write::SimpleFileOptions};
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, contents) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn install_activate_settings_and_fallback_survive_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pulse.db");
        let store = ThemeStore::open(&path).unwrap();
        let archive = theme_zip(&[
            ("komari-theme.json", br#"{"name":"Test","short":"test","version":"1.0.0","configuration":{"type":"managed","data":[{"key":"title","default":"Hello"}]}}"#),
            ("dist/index.html", b"<html>Theme</html>"),
            ("dist/assets/app.js", b"console.log('ok')"),
        ]);
        assert_eq!(store.install(&archive).unwrap().short, "test");
        assert_eq!(store.list().themes.len(), 2);
        store.activate("test").unwrap();
        assert_eq!(store.active(), "test");
        assert_eq!(store.settings()["title"], "Hello");
        store
            .save_settings("test", &json!({"title":"Changed"}))
            .unwrap();
        assert_eq!(store.settings()["title"], "Changed");
        assert_eq!(store.read_asset("").unwrap().1, b"<html>Theme</html>");
        assert!(store.read_asset("../pulse.db").is_none());
        let restored = ThemeStore::open(&path).unwrap();
        assert_eq!(restored.active(), "test");
        restored.activate("emerald").unwrap();
        assert!(restored.read_asset("").is_none());
    }

    #[test]
    fn luminaplus_sized_archive_is_within_bounded_entry_limit() {
        use zip::{ZipWriter, write::SimpleFileOptions};
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        writer.start_file("komari-theme.json", options).unwrap();
        writer.write_all(br#"{"name":"LuminaPlus","short":"LuminaPlus","version":"1.3.5","preview":"preview.png"}"#).unwrap();
        writer.start_file("preview.png", options).unwrap();
        writer.write_all(b"preview").unwrap();
        writer.start_file("dist/index.html", options).unwrap();
        writer.write_all(b"<html>Theme</html>").unwrap();
        for index in 0..319 {
            writer
                .start_file(format!("dist/assets/{index}.svg"), options)
                .unwrap();
            writer.write_all(b"<svg/>").unwrap();
        }
        let archive = writer.finish().unwrap().into_inner();
        let directory = tempfile::tempdir().unwrap();
        let store = ThemeStore::open(&directory.path().join("pulse.db")).unwrap();
        assert_eq!(store.install(&archive).unwrap().short, "LuminaPlus");
    }

    #[test]
    fn bundled_luminaplus_package_installs_and_serves_assets() {
        let archive = include_bytes!("../../../compat/luminaplus/LuminaPlus-v1.3.5-pulse.zip");
        let directory = tempfile::tempdir().unwrap();
        let store = ThemeStore::open(&directory.path().join("pulse.db")).unwrap();
        let manifest = store.install(archive).unwrap();
        assert_eq!(manifest.short, "LuminaPlus");
        store.activate("LuminaPlus").unwrap();
        let (served, html) = store.read_asset("").unwrap();
        assert_eq!(served, "index.html");
        assert!(std::str::from_utf8(&html).unwrap().contains("<html"));
    }

    #[test]
    fn invalid_archive_cannot_escape_theme_directory() {
        let directory = tempfile::tempdir().unwrap();
        let store = ThemeStore::open(&directory.path().join("pulse.db")).unwrap();
        let archive = theme_zip(&[
            (
                "komari-theme.json",
                br#"{"name":"Bad","short":"bad","version":"1"}"#,
            ),
            ("dist/index.html", b"<html></html>"),
            ("../escape", b"bad"),
        ]);
        assert!(store.install(&archive).is_err());
        assert!(!directory.path().join("escape").exists());
        assert!(!store.installed("bad"));
    }
}
