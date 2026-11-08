//! Sessions : tout ce qui est enregistré pendant une séance (captures, tableaux, vidéos)
//! va dans un même dossier, `Documents\Clicksy\AAAA-MM-JJ HHhMM`.
//!
//! Le dossier n'est créé qu'au premier élément enregistré. Une nouvelle session commence
//! au démarrage de Clicksy, chaque jour, ou à la demande (« Nouvelle session »).

use fx_core::Shared;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{FOLDERID_Documents, FOLDERID_Pictures, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

/// Dossier de la session en cours et sa date (année, mois, jour).
static CURRENT: Mutex<Option<(PathBuf, (u16, u16, u16))>> = Mutex::new(None);

fn known_folder(id: &windows::core::GUID, fallback: &str) -> PathBuf {
    unsafe {
        if let Ok(p) = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            if !s.is_empty() {
                return PathBuf::from(s);
            }
        }
    }
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(fallback)).unwrap_or_else(std::env::temp_dir)
}

/// Dossier qui contient toutes les sessions.
pub fn root(shared: &Shared) -> PathBuf {
    let custom = shared.config.load().session.folder.trim().to_string();
    if custom.is_empty() {
        known_folder(&FOLDERID_Documents, "Documents").join("Clicksy")
    } else {
        PathBuf::from(custom)
    }
}

pub fn enabled(shared: &Shared) -> bool {
    shared.config.load().session.enabled
}

/// Dossier de la session en cours, créé si besoin.
pub fn dir(shared: &Shared) -> Result<PathBuf, String> {
    let t = unsafe { GetLocalTime() };
    let today = (t.wYear, t.wMonth, t.wDay);
    let mut cur = CURRENT.lock().unwrap();
    if let Some((d, day)) = cur.as_ref() {
        if *day == today && d.starts_with(root(shared)) {
            std::fs::create_dir_all(d).map_err(|e| format!("dossier {} : {e}", d.display()))?;
            return Ok(d.clone());
        }
    }
    let base = root(shared).join(format!("{:04}-{:02}-{:02} {:02}h{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute));
    let d = unique(&base, "");
    std::fs::create_dir_all(&d).map_err(|e| format!("dossier {} : {e}", d.display()))?;
    *cur = Some((d.clone(), today));
    Ok(d)
}

/// Dossier de la session en cours, s'il existe déjà.
pub fn current() -> Option<PathBuf> {
    CURRENT.lock().unwrap().as_ref().map(|(d, _)| d.clone())
}

/// Le prochain élément enregistré ouvrira un nouveau dossier.
pub fn new_session(shared: &Shared) {
    *CURRENT.lock().unwrap() = None;
    shared.banner("Nouvelle session : les prochains éléments iront dans un nouveau dossier");
}

/// Ouvre le dossier de la session (ou, s'il n'y en a pas encore, celui des sessions).
pub fn open(shared: &Shared) {
    let d = current().filter(|d| d.exists()).unwrap_or_else(|| root(shared));
    let _ = std::fs::create_dir_all(&d);
    let _ = std::process::Command::new("explorer.exe").arg(&d).spawn();
}

fn clock() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:02}-{:02}-{:02}", t.wHour, t.wMinute, t.wSecond)
}

fn stamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// `base` + `ext` s'il est libre, sinon `base (2)` + `ext`, etc.
fn unique(base: &Path, ext: &str) -> PathBuf {
    let first = PathBuf::from(format!("{}{ext}", base.display()));
    if !first.exists() {
        return first;
    }
    (2..1000)
        .map(|i| PathBuf::from(format!("{} ({i}){ext}", base.display())))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Chemin d'une nouvelle image (`kind` : « Capture », « Tableau »).
pub fn image_path(shared: &Shared, kind: &str) -> Result<PathBuf, String> {
    if enabled(shared) {
        let d = dir(shared)?;
        return Ok(unique(&d.join(format!("{kind} {}", clock())), ".png"));
    }
    let custom = shared.config.load().capture.folder.trim().to_string();
    let d = if custom.is_empty() { known_folder(&FOLDERID_Pictures, "Pictures").join("Clicksy") } else { PathBuf::from(custom) };
    std::fs::create_dir_all(&d).map_err(|e| format!("dossier {} : {e}", d.display()))?;
    Ok(unique(&d.join(format!("{kind} {}", stamp())), ".png"))
}

/// Dossier parent et nom du dossier d'un nouvel enregistrement vidéo.
pub fn video_place(shared: &Shared) -> Result<(PathBuf, String), String> {
    if enabled(shared) {
        let d = dir(shared)?;
        let name = unique(&d.join(format!("Vidéo {}", clock())), "");
        return Ok((d, name.file_name().unwrap().to_string_lossy().into_owned()));
    }
    let parent = crate::recorder::videos_dir(shared);
    let name = unique(&parent.join(format!("Enregistrement {}", stamp())), "");
    Ok((parent, name.file_name().unwrap().to_string_lossy().into_owned()))
}
