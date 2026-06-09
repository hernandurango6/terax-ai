use arboard::Clipboard;
use png::{BitDepth, ColorType, Encoder};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static CLIPBOARD_SEQ: AtomicU64 = AtomicU64::new(0);

/// Drop captures older than this on each new paste.
const CLIPBOARD_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Hard cap so a heavy screenshot day cannot grow the cache without bound.
const CLIPBOARD_MAX_FILES: usize = 200;

fn clipboard_cache_dir() -> Result<PathBuf, String> {
    let base = dirs::cache_dir().ok_or_else(|| "cache dir unavailable".to_string())?;
    let dir = base.join("terax").join("clipboard");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn write_rgba_png(path: &Path, width: usize, height: usize, rgba: &[u8]) -> Result<(), String> {
    let w = u32::try_from(width).map_err(|_| "image width out of range".to_string())?;
    let h = u32::try_from(height).map_err(|_| "image height out of range".to_string())?;
    let mut file = File::create(path).map_err(|e| e.to_string())?;
    {
        let mut encoder = Encoder::new(&mut file, w, h);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

fn unique_clipboard_filename() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = CLIPBOARD_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("clipboard_{ms}_{seq}.png")
}

#[cfg(windows)]
fn to_terminal_path(path: &Path) -> Result<String, String> {
    let canon = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let s = canon.to_string_lossy();
    let stripped = s.strip_prefix(r"\\?\").unwrap_or(&s);
    Ok(stripped.replace('/', "\\"))
}

#[cfg(not(windows))]
fn to_terminal_path(path: &Path) -> Result<String, String> {
    let canon = fs::canonicalize(path).map_err(|e| e.to_string())?;
    Ok(crate::modules::fs::to_canon(&canon))
}

fn is_clipboard_image(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("clipboard_") && n.ends_with(".png"))
}

fn prune_old_clipboard_images_at(
    dir: &Path,
    now: SystemTime,
    max_age: Duration,
    max_files: usize,
) -> Result<(), String> {
    let mut entries: Vec<(PathBuf, SystemTime)> = Vec::new();

    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_file() || !is_clipboard_image(&path) {
            continue;
        }
        let modified = entry
            .metadata()
            .map_err(|e| e.to_string())?
            .modified()
            .unwrap_or(UNIX_EPOCH);
        entries.push((path, modified));
    }

    for (path, modified) in &entries {
        if now.duration_since(*modified).unwrap_or_default() > max_age {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }

    let mut surviving: Vec<(PathBuf, SystemTime)> = entries
        .into_iter()
        .filter(|(path, _)| path.exists())
        .collect();

    if surviving.len() > max_files {
        surviving.sort_by_key(|(_, modified)| *modified);
        let excess = surviving.len() - max_files;
        for (path, _) in surviving.into_iter().take(excess) {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

fn prune_old_clipboard_images(dir: &Path) {
    if let Err(e) = prune_old_clipboard_images_at(
        dir,
        SystemTime::now(),
        CLIPBOARD_MAX_AGE,
        CLIPBOARD_MAX_FILES,
    ) {
        log::warn!("clipboard cache prune failed: {e}");
    }
}

/// When the OS clipboard holds a bitmap (e.g. Win+Shift+S), write PNG to cache and
/// return its path. `None` means no image is available (caller should paste text).
#[tauri::command]
pub fn clipboard_image_to_file() -> Result<Option<String>, String> {
    let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
    let image = match clipboard.get_image() {
        Ok(img) => img,
        Err(arboard::Error::ContentNotAvailable) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };

    if image.width == 0 || image.height == 0 {
        return Ok(None);
    }

    let dir = clipboard_cache_dir()?;
    let path = dir.join(unique_clipboard_filename());
    write_rgba_png(&path, image.width, image.height, &image.bytes)?;
    prune_old_clipboard_images(&dir);
    Ok(Some(to_terminal_path(&path)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn unique_clipboard_filenames_differ() {
        let a = unique_clipboard_filename();
        let b = unique_clipboard_filename();
        assert_ne!(a, b);
        assert!(a.ends_with(".png"));
    }

    #[test]
    fn prune_skips_non_clipboard_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("clipboard_old_0.png"), b"x").unwrap();
        fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(10));
        prune_old_clipboard_images_at(
            dir.path(),
            SystemTime::now(),
            Duration::ZERO,
            CLIPBOARD_MAX_FILES,
        )
        .unwrap();
        assert!(!dir.path().join("clipboard_old_0.png").exists());
        assert!(dir.path().join("notes.txt").exists());
    }

    #[test]
    fn prune_caps_file_count() {
        let dir = TempDir::new().unwrap();
        for i in 0..5 {
            fs::write(
                dir.path().join(format!("clipboard_{i}_0.png")),
                b"x",
            )
            .unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        prune_old_clipboard_images_at(
            dir.path(),
            SystemTime::now(),
            Duration::from_secs(365 * 24 * 60 * 60),
            2,
        )
        .unwrap();
        let clipboard_count = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| is_clipboard_image(&e.path()))
            .count();
        assert_eq!(clipboard_count, 2);
    }

    #[test]
    fn write_rgba_png_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.png");
        write_rgba_png(&path, 2, 2, &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255])
            .unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(bytes.len() > 32);
    }
}