//! Icône de la zone de notification et son menu (thread principal).
//!
//! Prototype en Win32 natif ; la fenêtre de réglages (Tauri) viendra s'y greffer.

use fx_bus::{Action, SpotlightModeId};
use fx_core::{Config, Shared, SpotlightMode};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_TRAY: u32 = WM_APP + 1;
const WM_REFRESH: u32 = WM_APP + 2;
const WM_OPEN_SETTINGS: u32 = WM_APP + 3;
const TRAY_ID: u32 = 1;

const ID_SETTINGS: usize = 2;
const ID_ENABLED: usize = 1;
const ID_SPOT_OFF: usize = 10;
const ID_SPOT_CIRCLE: usize = 11;
const ID_SPOT_HBAND: usize = 12;
const ID_SPOT_VBAND: usize = 13;
const ID_RING: usize = 20;
const ID_PULSE: usize = 21;
const ID_CLICKS: usize = 30;
const ID_SOUND: usize = 31;
const ID_TRAIL: usize = 32;
const ID_LASER: usize = 33;
const ID_DRAW: usize = 34;
const ID_TOOLBAR: usize = 35;
const ID_BOARD: usize = 36;
const ID_CAPTURE: usize = 37;
const ID_RECORD: usize = 38;
const ID_OPEN_DIR: usize = 40;
const ID_RELOAD: usize = 41;
const ID_QUIT: usize = 99;

static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

/// Demande à l'icône de se mettre à jour (appelable depuis n'importe quel thread).
pub fn notify_config_changed() {
    let h = TRAY_HWND.load(Ordering::Acquire);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut _)), WM_REFRESH, WPARAM(0), LPARAM(0));
        }
    }
}

pub struct Callbacks {
    pub open_settings: Box<dyn Fn() + Send + Sync>,
    pub quit: Box<dyn Fn() + Send + Sync>,
}

static CALLBACKS: OnceLock<Callbacks> = OnceLock::new();

/// Démarre l'icône sur son propre thread (le thread principal appartient à Tauri).
pub fn start(shared: Arc<Shared>, callbacks: Callbacks) {
    let _ = CALLBACKS.set(callbacks);
    std::thread::Builder::new()
        .name("clicksy-tray".into())
        .spawn(move || run(shared))
        .expect("thread de l'icône");
}

/// Retire l'icône (à la fermeture de l'app).
pub fn shutdown() {
    SHUTTING_DOWN.store(true, Ordering::Release);
    let h = TRAY_HWND.swap(0, Ordering::AcqRel);
    if h != 0 {
        unsafe {
            remove_icon(HWND(h as *mut _));
            let _ = PostMessageW(Some(HWND(h as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// Actions déclenchables en ligne de commande (`clicksy.exe --action dessin`),
/// par exemple depuis un Stream Deck ou un script. 0 = ouvrir les réglages.
pub const CLI_ACTIONS: [(&str, fn() -> Action); 15] = [
    ("tout", || Action::ToggleAll),
    ("spotlight", || Action::ToggleSpotlight),
    ("anneau", || Action::ToggleRing),
    ("clics", || Action::ToggleClicks),
    ("trainee", || Action::ToggleTrail),
    ("laser", || Action::ToggleLaser),
    ("dessin", || Action::ToggleDraw),
    ("pulsation", || Action::TogglePulse),
    ("son", || Action::ToggleClickSound),
    ("tableau", || Action::CycleBoard),
    ("blanc", || Action::SetBoard(1)),
    ("noir", || Action::SetBoard(2)),
    ("capture", || Action::Screenshot),
    ("barre", || Action::ToggleToolbar),
    ("video", || Action::ToggleRecord),
];

/// Code (1-based) d'une action nommée, pour [`signal_running_instance`].
pub fn cli_action_code(name: &str) -> Option<usize> {
    let n = name.trim().to_lowercase().replace('î', "i");
    CLI_ACTIONS.iter().position(|(k, _)| *k == n).map(|i| i + 1)
}

/// Transmet à l'instance déjà lancée une action (code) ou, avec 0, l'ouverture des réglages.
/// Renvoie `false` si aucune instance n'est trouvée.
pub fn signal_running_instance(code: usize) -> bool {
    unsafe {
        match FindWindowW(w!("ClicksyTray"), None) {
            Ok(h) if !h.is_invalid() => PostMessageW(Some(h), WM_OPEN_SETTINGS, WPARAM(code), LPARAM(0)).is_ok(),
            _ => false,
        }
    }
}

fn open_settings() {
    if let Some(c) = CALLBACKS.get() {
        (c.open_settings)();
    }
}

/// Boucle de messages du thread de l'icône.
fn run(shared: Arc<Shared>) {
    let _ = SHARED.set(shared);
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("ClicksyTray");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Release);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Clicksy"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .expect("fenêtre de l'icône");
        TRAY_HWND.store(hwnd.0 as isize, Ordering::Release);
        add_icon(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        remove_icon(hwnd);
        TRAY_HWND.store(0, Ordering::Release);
    }
    // Fermeture demandée depuis le menu : on prévient l'app. (Si c'est l'app qui ferme, SHUTTING_DOWN est déjà levé.)
    if !SHUTTING_DOWN.swap(true, Ordering::AcqRel) {
        if let Some(c) = CALLBACKS.get() {
            (c.quit)();
        }
    }
}

fn config() -> Arc<Config> {
    SHARED.get().unwrap().config.load_full()
}

fn tooltip(cfg: &Config) -> String {
    if cfg.enabled {
        format!("Clicksy : actif (spotlight : {})", cfg.spotlight.mode.label_fr().to_lowercase())
    } else {
        "Clicksy : en pause".to_string()
    }
}

unsafe fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let cfg = config();
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: WM_TRAY,
        hIcon: make_icon(cfg.enabled),
        ..Default::default()
    };
    copy_wide(&mut nid.szTip, &tooltip(&cfg));
    nid
}

unsafe fn add_icon(hwnd: HWND) {
    let mut nid = icon_data(hwnd);
    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
    let _ = DestroyIcon(nid.hIcon);
}

unsafe fn refresh_icon(hwnd: HWND) {
    let nid = icon_data(hwnd);
    let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    let _ = DestroyIcon(nid.hIcon);
}

unsafe fn remove_icon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

fn copy_wide(dst: &mut [u16], s: &str) {
    let mut n = 0;
    for (d, c) in dst.iter_mut().zip(s.encode_utf16()) {
        *d = c;
        n += 1;
    }
    if n >= dst.len() {
        n = dst.len() - 1;
    }
    dst[n] = 0;
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            // NOTIFYICON_VERSION_4 : l'événement est dans le mot bas de lParam.
            let event = (lparam.0 & 0xFFFF) as u32;
            // Clic gauche (ou Entrée au clavier) : réglages. Clic droit : menu.
            if event == NIN_SELECT || event == NIN_SELECT + 1 {
                open_settings();
            } else if event == WM_CONTEXTMENU {
                show_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_OPEN_SETTINGS => {
            match wparam.0 {
                0 => open_settings(),
                code => {
                    if let Some((_, make)) = CLI_ACTIONS.get(code - 1) {
                        fx_bus::action(make());
                    }
                }
            }
            LRESULT(0)
        }
        WM_REFRESH => {
            refresh_icon(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if m == TASKBAR_CREATED.load(Ordering::Acquire) && m != 0 => {
            // L'Explorateur a redémarré : on remet l'icône.
            add_icon(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn hk_label(label: &str, hk: Option<fx_core::Hotkey>) -> String {
    match hk {
        Some(h) => format!("{label}\t{h}"),
        None => label.to_string(),
    }
}

unsafe fn append(menu: HMENU, id: usize, label: &str, checked: bool, enabled: bool) {
    let mut flags = MF_STRING;
    if checked {
        flags |= MF_CHECKED;
    }
    if !enabled {
        flags |= MF_GRAYED;
    }
    let wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
    let _ = AppendMenuW(menu, flags, id, PCWSTR(wide.as_ptr()));
}

unsafe fn show_menu(hwnd: HWND) {
    let cfg = config();
    let Ok(menu) = CreatePopupMenu() else { return };
    let Ok(spot) = CreatePopupMenu() else { return };

    append(menu, ID_SETTINGS, "Réglages…", false, true);
    let _ = SetMenuDefaultItem(menu, ID_SETTINGS as u32, 0);
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    append(menu, ID_ENABLED, &hk_label("Effets activés", cfg.hotkeys.toggle_all), cfg.enabled, true);
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);

    let m = cfg.spotlight.mode;
    append(spot, ID_SPOT_OFF, "Désactivé", m == SpotlightMode::Off, true);
    append(spot, ID_SPOT_CIRCLE, "Cercle", m == SpotlightMode::Circle, true);
    append(spot, ID_SPOT_HBAND, "Bande horizontale", m == SpotlightMode::HBand, true);
    append(spot, ID_SPOT_VBAND, "Bande verticale", m == SpotlightMode::VBand, true);
    let spot_label: Vec<u16> = hk_label("Spotlight", cfg.hotkeys.cycle_spotlight)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let _ = AppendMenuW(menu, MF_POPUP, spot.0 as usize, PCWSTR(spot_label.as_ptr()));

    append(menu, ID_RING, &hk_label("Anneau autour du curseur", cfg.hotkeys.toggle_ring), cfg.ring.enabled, true);
    append(menu, ID_PULSE, "Pulsation de l'anneau", cfg.ring.pulse, cfg.ring.enabled);
    append(menu, ID_CLICKS, &hk_label("Effets de clic", cfg.hotkeys.toggle_clicks), cfg.clicks.enabled, true);
    append(menu, ID_SOUND, "Son de clic", cfg.clicks.sound, cfg.clicks.enabled);
    append(menu, ID_TRAIL, &hk_label("Traînée du curseur", cfg.hotkeys.toggle_trail), cfg.trail.enabled, true);
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    let modes = &SHARED.get().unwrap().modes;
    append(menu, ID_LASER, &hk_label("Pointeur laser", cfg.hotkeys.toggle_laser), modes.laser.load(Ordering::Acquire), true);
    append(menu, ID_DRAW, &hk_label("Mode dessin", cfg.hotkeys.toggle_draw), modes.draw.load(Ordering::Acquire), true);
    append(menu, ID_BOARD, &hk_label("Tableau blanc / noir", cfg.hotkeys.cycle_board), modes.board.load(Ordering::Acquire) != 0, true);
    append(menu, ID_CAPTURE, &hk_label("Capture d'écran", cfg.hotkeys.screenshot), false, true);
    let rec_label = if modes.recording.load(Ordering::Acquire) { "Arrêter l'enregistrement" } else { "Enregistrer l'écran" };
    append(menu, ID_RECORD, &hk_label(rec_label, cfg.hotkeys.toggle_record), modes.recording.load(Ordering::Acquire), true);
    append(menu, ID_TOOLBAR, &hk_label("Barre d'outils", cfg.hotkeys.toggle_toolbar), cfg.toolbar.visible, true);
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    append(menu, ID_OPEN_DIR, "Ouvrir le dossier de configuration", false, true);
    append(menu, ID_RELOAD, "Recharger la configuration", false, true);
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    append(menu, ID_QUIT, "Quitter Clicksy", false, true);

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    // Indispensable pour que le menu se ferme quand on clique ailleurs.
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RIGHTALIGN,
        pt.x,
        pt.y,
        None,
        hwnd,
        None,
    );
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu); // détruit aussi le sous-menu

    match cmd.0 as usize {
        ID_SETTINGS => open_settings(),
        ID_ENABLED => fx_bus::action(Action::ToggleAll),
        ID_SPOT_OFF => fx_bus::action(Action::SetSpotlight(SpotlightModeId::Off)),
        ID_SPOT_CIRCLE => fx_bus::action(Action::SetSpotlight(SpotlightModeId::Circle)),
        ID_SPOT_HBAND => fx_bus::action(Action::SetSpotlight(SpotlightModeId::HBand)),
        ID_SPOT_VBAND => fx_bus::action(Action::SetSpotlight(SpotlightModeId::VBand)),
        ID_RING => fx_bus::action(Action::ToggleRing),
        ID_PULSE => fx_bus::action(Action::TogglePulse),
        ID_CLICKS => fx_bus::action(Action::ToggleClicks),
        ID_SOUND => fx_bus::action(Action::ToggleClickSound),
        ID_TRAIL => fx_bus::action(Action::ToggleTrail),
        ID_LASER => fx_bus::action(Action::ToggleLaser),
        ID_DRAW => fx_bus::action(Action::ToggleDraw),
        ID_BOARD => fx_bus::action(Action::CycleBoard),
        ID_CAPTURE => fx_bus::action(Action::Screenshot),
        ID_RECORD => fx_bus::action(Action::ToggleRecord),
        ID_TOOLBAR => fx_bus::action(Action::ToggleToolbar),
        ID_RELOAD => fx_bus::action(Action::ReloadConfig),
        ID_OPEN_DIR => open_data_dir(),
        ID_QUIT => {
            let _ = DestroyWindow(hwnd);
        }
        _ => {}
    }
}

unsafe fn open_data_dir() {
    let dir = SHARED.get().unwrap().data_dir.to_string_lossy().to_string();
    let wide: Vec<u16> = dir.encode_utf16().chain(std::iter::once(0)).collect();
    ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
}

/// Icône dessinée par programme : anneau jaune (actif) ou gris (en pause), point bleu au centre.
unsafe fn make_icon(active: bool) -> HICON {
    let size = GetSystemMetrics(SM_CXSMICON).max(16);
    let n = size as usize;
    let mut px = vec![0u32; n * n];
    let c = (n as f32 - 1.0) / 2.0;
    let r_out = n as f32 * 0.47;
    let r_in = n as f32 * 0.30;
    let r_dot = n as f32 * 0.14;
    let ring = if active { (0xFF, 0xC4, 0x00) } else { (0x9A, 0x9A, 0x9A) };
    let dot = if active { (0x1E, 0x90, 0xFF) } else { (0x6A, 0x6A, 0x6A) };
    for y in 0..n {
        for x in 0..n {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let cover = |inner: f32, outer: f32| ((outer - d + 0.5).clamp(0.0, 1.0)) * ((d - inner + 0.5).clamp(0.0, 1.0));
            let a_ring = cover(r_in, r_out);
            let a_dot = cover(-1.0, r_dot);
            let (col, a) = if a_dot > 0.0 { (dot, a_dot) } else { (ring, a_ring) };
            let a8 = (a * 255.0) as u32;
            // BGRA prémultiplié.
            let pm = |v: u8| (v as f32 * a) as u32;
            px[y * n + x] = (a8 << 24) | (pm(col.0) << 16) | (pm(col.1) << 8) | pm(col.2);
        }
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let Ok(color) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
        return LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
    };
    std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u32, n * n);
    let mask = CreateBitmap(size, size, 1, 1, None);
    let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
    let icon = CreateIconIndirect(&info).unwrap_or_default();
    let _ = DeleteObject(color.into());
    let _ = DeleteObject(mask.into());
    icon
}
