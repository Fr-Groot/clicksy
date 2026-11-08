// En version release, pas de console. En debug, la console affiche les messages de diagnostic.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod engine;
mod keys;
mod recorder;
mod session;
mod settings;
mod tray;

use fx_core::{paths, Shared};
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

fn main() {
    let t0 = std::time::Instant::now();
    unsafe {
        // Déjà déclaré dans le manifeste ; l'appel est une sécurité supplémentaire.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // `clicksy.exe --action dessin` : transmet une action à l'instance lancée (Stream Deck, scripts).
    let args: Vec<String> = std::env::args().collect();
    let cli_action = args
        .iter()
        .position(|a| a == "--action")
        .and_then(|i| args.get(i + 1))
        .map(|name| tray::cli_action_code(name).ok_or(name.clone()));

    // Une seule instance par session : la 2ᵉ transmet son action (ou ouvre les réglages), puis s'arrête.
    let _mutex = unsafe { CreateMutexW(None, true, w!("Local\\Clicksy.Instance")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        match cli_action {
            Some(Ok(code)) => {
                tray::signal_running_instance(code);
            }
            Some(Err(name)) => {
                let names: Vec<&str> = tray::CLI_ACTIONS.iter().map(|(k, _)| *k).collect();
                eprintln!("Action inconnue : {name}. Actions possibles : {}", names.join(", "));
            }
            None => {
                tray::signal_running_instance(0);
            }
        }
        return;
    }

    fx_bus::init();
    let dir = paths::data_dir();
    let (cfg, warning) = paths::load_config(&dir);
    if let Some(w) = &warning {
        eprintln!("{w}");
    }
    eprintln!("Clicksy : données dans {}", dir.display());
    let shared = Shared::new(cfg, dir);

    // Le moteur, le rendu et les hooks démarrent avant Tauri : les effets sont là tout de suite.
    let s = shared.clone();
    engine::start(shared.clone(), fx_bus::subscribe(), move || {
        tray::notify_config_changed();
        settings::notify_config_changed(&s.config.load());
    });
    fx_overlay::start(shared.clone());
    fx_input::start(shared.clone());

    let tray_shared = shared.clone();
    let app = tauri::Builder::default()
        .manage(shared)
        .invoke_handler(tauri::generate_handler![
            settings::get_config,
            settings::default_config,
            settings::set_config,
            settings::run_action,
            settings::session_info,
            settings::open_data_dir,
            settings::data_dir,
        ])
        .setup(move |app| {
            settings::init(app.handle().clone());
            tray::start(
                tray_shared.clone(),
                tray::Callbacks { open_settings: Box::new(settings::open), quit: Box::new(settings::quit) },
            );
            eprintln!("Clicksy : démarré en {} ms", t0.elapsed().as_millis());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("initialisation de Tauri");

    app.run(|_app, event| match event {
        // Fermer la fenêtre de réglages ne quitte pas Clicksy : seul « Quitter » le fait.
        tauri::RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
        tauri::RunEvent::Exit => {
            tray::shutdown();
            fx_input::stop();
            fx_overlay::stop();
        }
        _ => {}
    });
}
