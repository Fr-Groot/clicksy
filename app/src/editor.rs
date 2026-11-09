//! Fenêtre de l'éditeur vidéo (Tauri) et ses commandes : liste des enregistrements,
//! chargement d'un projet, sauvegarde des réglages d'édition, export en tâche de fond.

use fx_core::Shared;
use fx_record::export::{Edit, ProjectView};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "editor";
static EXPORTING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Ouvre l'éditeur, sur le projet `dir` s'il est donné (sinon sur le plus récent).
/// Appelable depuis n'importe quel thread.
pub fn open(dir: Option<PathBuf>) {
    let Some(app) = crate::settings::app() else { return };
    let dir_s = dir.map(|d| d.display().to_string()).unwrap_or_default();
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if !dir_s.is_empty() {
            let _ = app.emit_to(LABEL, "editor-open", dir_s);
        }
        return;
    }
    let shared = app.state::<Arc<Shared>>();
    let url = if dir_s.is_empty() { "editor.html".to_string() } else { format!("editor.html?dir={}", encode(&dir_s)) };
    let built = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App(url.into()))
        .title(fx_core::t("Clicksy – Éditeur vidéo"))
        .inner_size(1180.0, 780.0)
        .min_inner_size(820.0, 560.0)
        .center()
        .data_directory(shared.data_dir.join("webview"))
        .build();
    if let Err(e) = built {
        eprintln!("Clicksy : impossible d'ouvrir l'éditeur : {e}");
    }
}

/// Encodage pour une URL (chemins Windows avec espaces et accents).
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Dossiers où chercher les enregistrements : sessions et ancien dossier Vidéos\Clicksy.
fn roots(shared: &Shared) -> Vec<PathBuf> {
    vec![crate::session::root(shared), crate::recorder::videos_dir(shared)]
}

#[derive(Serialize)]
pub struct ProjectItem {
    dir: String,
    /// « Excel niveau 1 › 2026-11-09 14h30 › Vidéo 14-31-05 » (chemin sous le dossier racine).
    label: String,
}

#[tauri::command]
pub fn editor_projects(shared: State<Arc<Shared>>) -> Vec<ProjectItem> {
    let roots = roots(&shared);
    fx_record::export::find_projects(&roots)
        .into_iter()
        .take(200)
        .map(|(d, _)| {
            let rel = roots.iter().find_map(|r| d.strip_prefix(r).ok()).map(Path::to_path_buf).unwrap_or_else(|| d.clone());
            let label = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join(" › ");
            ProjectItem { dir: d.display().to_string(), label }
        })
        .collect()
}

#[tauri::command]
pub fn editor_load(app: AppHandle, dir: String) -> Result<ProjectView, String> {
    let path = PathBuf::from(&dir);
    // La fenêtre a besoin de lire la vidéo et le son de ce dossier (protocole asset).
    let _ = app.asset_protocol_scope().allow_directory(&path, false);
    fx_record::export::load(&path)
}

#[tauri::command]
pub fn editor_save(dir: String, edit: Edit) -> Result<(), String> {
    fx_record::export::save(Path::new(&dir), &edit)
}

#[derive(Clone, Serialize)]
struct Progress {
    dir: String,
    progress: f32,
}

#[derive(Clone, Serialize)]
struct Done {
    dir: String,
    path: Option<String>,
    error: Option<String>,
}

/// Lance l'export en tâche de fond. Avancement : événements « export-progress » puis « export-done ».
#[tauri::command]
pub fn editor_export(app: AppHandle, dir: String, edit: Edit) -> Result<String, String> {
    if EXPORTING.swap(true, Ordering::AcqRel) {
        return Err(fx_core::t("Un export est déjà en cours").into());
    }
    CANCEL.store(false, Ordering::Release);
    let base = PathBuf::from(&dir);
    let name = fx_core::t("Vidéo finale");
    let mut out = base.join(format!("{name}.mp4"));
    let mut i = 2;
    while out.exists() {
        out = base.join(format!("{name} ({i}).mp4"));
        i += 1;
    }
    let out_s = out.display().to_string();
    let _ = fx_record::export::save(&base, &edit);
    std::thread::spawn(move || {
        let d = dir.clone();
        let a = app.clone();
        let res = fx_record::export::export(&base, &edit, &out, &CANCEL, move |p| {
            let _ = a.emit_to(LABEL, "export-progress", Progress { dir: d.clone(), progress: p });
        });
        EXPORTING.store(false, Ordering::Release);
        let done = match res {
            Ok(()) => Done { dir, path: Some(out.display().to_string()), error: None },
            Err(e) => Done { dir, path: None, error: Some(e) },
        };
        let _ = app.emit_to(LABEL, "export-done", done);
    });
    Ok(out_s)
}

#[tauri::command]
pub fn editor_cancel() {
    CANCEL.store(true, Ordering::Release);
}

/// Ouvre l'Explorateur sur un fichier (sélectionné) ou un dossier.
#[tauri::command]
pub fn reveal(path: String) {
    let p = PathBuf::from(&path);
    let _ = if p.is_file() {
        std::process::Command::new("explorer.exe").arg(format!("/select,{}", p.display())).spawn()
    } else {
        std::process::Command::new("explorer.exe").arg(&p).spawn()
    };
}

/// Ouvre un fichier avec l'application par défaut (lecteur vidéo, navigateur…).
#[tauri::command]
pub fn open_file(path: String) {
    let _ = std::process::Command::new("explorer.exe").arg(&path).spawn();
}
