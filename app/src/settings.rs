//! Fenêtre de réglages (Tauri) : créée à la première ouverture seulement,
//! pour garder un démarrage rapide et ne pas charger WebView2 inutilement.

use fx_core::{paths, Config, Shared};
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "settings";
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init(app: AppHandle) {
    let _ = APP.set(app);
}

/// Ouvre (ou ramène au premier plan) la fenêtre de réglages. Appelable depuis n'importe quel thread.
pub fn open() {
    let Some(app) = APP.get() else { return };
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let shared = app.state::<Arc<Shared>>();
    // Mode portable : les données de WebView2 restent dans notre dossier, pas dans le profil Windows.
    let webview_dir = shared.data_dir.join("webview");
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title(fx_core::t("Clicksy – Réglages"))
        .inner_size(860.0, 640.0)
        .min_inner_size(640.0, 480.0)
        .center()
        .data_directory(webview_dir)
        .build();
    if let Err(e) = built {
        eprintln!("Clicksy : impossible d'ouvrir les réglages : {e}");
    }
}

/// Prévient la fenêtre de réglages qu'un raccourci ou l'icône a changé la config.
pub fn notify_config_changed(cfg: &Config) {
    if let Some(app) = APP.get() {
        let _ = app.emit_to(LABEL, "config-changed", cfg);
    }
}

pub fn quit() {
    if let Some(app) = APP.get() {
        app.exit(0);
    }
}

#[tauri::command]
pub fn get_config(shared: State<Arc<Shared>>) -> Config {
    (**shared.config.load()).clone()
}

#[tauri::command]
pub fn default_config() -> Config {
    Config::default()
}

/// Applique une config venant de l'interface (aperçu en direct).
/// `save` : écrire aussi sur le disque (à la fin d'un glissement de curseur, pas pendant).
#[tauri::command]
pub fn set_config(shared: State<Arc<Shared>>, config: Config, save: bool) -> Result<Config, String> {
    shared.update_config(|c| *c = config);
    let applied = (**shared.config.load()).clone();
    if save {
        paths::save_config(&shared.data_dir, &applied).map_err(|e| format!("Enregistrement impossible : {e}"))?;
    }
    crate::tray::notify_config_changed();
    Ok(applied)
}

#[tauri::command]
pub fn open_data_dir(shared: State<Arc<Shared>>) {
    let _ = std::process::Command::new("explorer.exe").arg(&shared.data_dir).spawn();
}

#[tauri::command]
pub fn data_dir(shared: State<Arc<Shared>>) -> String {
    shared.data_dir.display().to_string()
}

/// Boutons de l'interface qui déclenchent une action (« Nouvelle session »…).
#[tauri::command]
pub fn run_action(name: String) -> Result<(), String> {
    let action = match name.as_str() {
        "new_session" => fx_bus::Action::NewSession,
        "open_session" => fx_bus::Action::OpenSessionFolder,
        _ => return Err(format!("action inconnue : {name}")),
    };
    fx_bus::action(action);
    Ok(())
}

/// Dossier de la session en cours (vide si rien n'a encore été enregistré) et dossier des sessions.
#[tauri::command]
pub fn session_info(shared: State<Arc<Shared>>) -> (String, String) {
    let current = crate::session::current().map(|d| d.display().to_string()).unwrap_or_default();
    (current, crate::session::root(&shared).display().to_string())
}

/// Change la formation en cours (nom libre, vide : aucune).
#[tauri::command]
pub fn set_formation(shared: State<Arc<Shared>>, name: String) -> Config {
    crate::session::set_formation(&shared, &name);
    crate::tray::notify_config_changed();
    (**shared.config.load()).clone()
}

/// Langue effective de l'interface (« fr » ou « en »), « Auto » résolu selon Windows.
#[tauri::command]
pub fn ui_language() -> &'static str {
    fx_core::i18n::code()
}
