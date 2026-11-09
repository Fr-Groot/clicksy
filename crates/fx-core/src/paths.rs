//! Emplacement des données : `data\` à côté de l'exécutable si on peut y écrire
//! (mode portable), sinon `%APPDATA%\Clicksy` (installation dans Program Files).

use crate::Config;
use std::io;
use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    if let Some(dir) = portable_dir() {
        if is_writable(&dir) {
            return dir;
        }
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join("Clicksy");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn portable_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("data"))
}

fn is_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-test");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

/// Charge la config ; si elle est absente, écrit les valeurs par défaut.
/// Si le fichier est invalide, il est renommé en `.invalide` et on repart des défauts.
pub fn load_config(dir: &Path) -> (Config, Option<String>) {
    let path = config_path(dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Config>(&text) {
            Ok(mut c) => {
                c.sanitize();
                (c, None)
            }
            Err(e) => {
                let _ = std::fs::rename(&path, path.with_extension("json.invalide"));
                let mut c = Config::default();
                c.sanitize();
                let _ = save_config(dir, &c);
                (c, Some(format!("config.json invalide ({e}) : valeurs par défaut restaurées.")))
            }
        },
        Err(_) => {
            let mut c = Config::default();
            c.sanitize();
            let _ = save_config(dir, &c);
            (c, None)
        }
    }
}

/// Écriture atomique (fichier temporaire puis renommage).
pub fn save_config(dir: &Path, c: &Config) -> io::Result<()> {
    let path = config_path(dir);
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(c).map_err(io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}
