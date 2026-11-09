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

/// Dossier qui reçoit les sessions : celui de la formation en cours, ou la racine.
pub fn parent(shared: &Shared) -> PathBuf {
    let formation = shared.config.load().session.formation.clone();
    if formation.is_empty() {
        root(shared)
    } else {
        root(shared).join(formation)
    }
}

/// Dossier de la session en cours, créé si besoin.
pub fn dir(shared: &Shared) -> Result<PathBuf, String> {
    let t = unsafe { GetLocalTime() };
    let today = (t.wYear, t.wMonth, t.wDay);
    let parent = parent(shared);
    let mut cur = CURRENT.lock().unwrap();
    if let Some((d, day)) = cur.as_ref() {
        // Même jour et même formation : on continue dans le même dossier.
        if *day == today && d.parent() == Some(parent.as_path()) {
            std::fs::create_dir_all(d).map_err(|e| format!("{} : {e}", d.display()))?;
            return Ok(d.clone());
        }
    }
    let base = parent.join(format!("{:04}-{:02}-{:02} {:02}h{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute));
    let d = unique(&base, "");
    std::fs::create_dir_all(&d).map_err(|e| format!("{} : {e}", d.display()))?;
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
    shared.banner(fx_core::t("Nouvelle session : les prochains éléments iront dans un nouveau dossier"));
}

/// Change la formation en cours (vide : aucune) et la place en tête des récentes.
/// Les éléments suivants iront dans une nouvelle session, dans le dossier de cette formation.
pub fn set_formation(shared: &Shared, name: &str) {
    let name = fx_core::folder_name(name);
    shared.update_config(|c| {
        c.session.formation = name.clone();
        if !name.is_empty() {
            c.session.formations.retain(|f| !f.eq_ignore_ascii_case(&name));
            c.session.formations.insert(0, name.clone());
        }
    });
    let _ = fx_core::paths::save_config(&shared.data_dir, &shared.config.load_full());
    *CURRENT.lock().unwrap() = None;
    shared.banner(if name.is_empty() {
        fx_core::t("Aucune formation").to_string()
    } else {
        fx_core::tf("Formation : {}", &[&name])
    });
}

/// Ouvre le dossier de la session (ou, s'il n'y en a pas encore, celui de la formation).
pub fn open(shared: &Shared) {
    let d = current().filter(|d| d.exists()).unwrap_or_else(|| parent(shared));
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
    std::fs::create_dir_all(&d).map_err(|e| format!("{} : {e}", d.display()))?;
    Ok(unique(&d.join(format!("{kind} {}", stamp())), ".png"))
}

/// Dossier parent et nom du dossier d'un nouvel enregistrement vidéo.
pub fn video_place(shared: &Shared) -> Result<(PathBuf, String), String> {
    if enabled(shared) {
        let d = dir(shared)?;
        let name = unique(&d.join(format!("{} {}", fx_core::t("Vidéo"), clock())), "");
        return Ok((d, name.file_name().unwrap().to_string_lossy().into_owned()));
    }
    let parent = crate::recorder::videos_dir(shared);
    let name = unique(&parent.join(format!("{} {}", fx_core::t("Enregistrement"), stamp())), "");
    Ok((parent, name.file_name().unwrap().to_string_lossy().into_owned()))
}
