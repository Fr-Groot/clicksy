//! Pilotage de l'enregistrement vidéo : choix de la source, compte à rebours, démarrage, arrêt.

use fx_bus::{Action, MouseButton, RecordTarget};
use fx_core::Shared;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::{FOLDERID_Videos, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetCursorPos, GetWindowThreadProcessId, WindowFromPoint, GA_ROOT};

static RECORDING: Mutex<Option<fx_record::Recording>> = Mutex::new(None);
const COUNTDOWN: Duration = Duration::from_secs(3);

pub fn is_recording() -> bool {
    RECORDING.lock().unwrap().is_some()
}

pub fn videos_dir(shared: &Shared) -> PathBuf {
    let custom = shared.config.load().record.folder.trim().to_string();
    if !custom.is_empty() {
        return PathBuf::from(custom);
    }
    unsafe {
        if let Ok(p) = SHGetKnownFolderPath(&FOLDERID_Videos, KF_FLAG_DEFAULT, None) {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            if !s.is_empty() {
                return PathBuf::from(s).join("Clicksy");
            }
        }
    }
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Videos").join("Clicksy")).unwrap_or_else(std::env::temp_dir)
}

fn cursor() -> POINT {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    pt
}

fn monitor_at(pt: POINT) -> (HMONITOR, RECT) {
    unsafe {
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(hmon, &mut info);
        (hmon, info.rcMonitor)
    }
}

/// Fenêtre principale sous un point, en ignorant celles de Clicksy.
pub fn window_at(pt: POINT) -> Option<HWND> {
    unsafe {
        let hwnd = GetAncestor(WindowFromPoint(pt), GA_ROOT);
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (pid != GetCurrentProcessId()).then_some(hwnd)
    }
}

/// Rectangle (bureau) qui sera enregistré, pour le compte à rebours et le cadre rouge.
fn target_rect(t: RecordTarget) -> [i32; 4] {
    match t {
        RecordTarget::Screen => {
            let r = monitor_at(cursor()).1;
            [r.left, r.top, r.right, r.bottom]
        }
        RecordTarget::Window { hwnd } => fx_record_window_rect(hwnd).unwrap_or([0, 0, 0, 0]),
        RecordTarget::Region { left, top, right, bottom } => [left, top, right, bottom],
    }
}

fn fx_record_window_rect(hwnd: isize) -> Option<[i32; 4]> {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
    let mut r = RECT::default();
    unsafe {
        DwmGetWindowAttribute(HWND(hwnd as *mut _), DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32)
            .ok()?;
    }
    Some([r.left, r.top, r.right, r.bottom])
}

/// Bouton « Enregistrer » ou raccourci : compte à rebours puis démarrage.
pub fn start(shared: &Shared, target: RecordTarget) {
    if is_recording() || shared.fx.lock().unwrap().countdown.is_some() {
        return;
    }
    // L'écran est figé au moment du clic (la souris peut bouger pendant le compte à rebours).
    let target = match target {
        RecordTarget::Screen => {
            let r = monitor_at(cursor()).1;
            RecordTarget::Region { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
        }
        t => t,
    };
    if shared.config.load().record.countdown {
        shared.fx.lock().unwrap().countdown = Some((Instant::now(), target_rect(target)));
        shared.wake();
        std::thread::spawn(move || {
            std::thread::sleep(COUNTDOWN);
            fx_bus::action(Action::RecordGo(target));
        });
    } else {
        fx_bus::action(Action::RecordGo(target));
    }
}

pub fn go(shared: &Arc<Shared>, target: RecordTarget) {
    shared.fx.lock().unwrap().countdown = None;
    if is_recording() {
        return;
    }
    let cfg = shared.config.load_full();
    let rect = target_rect(target);
    let center = POINT { x: (rect[0] + rect[2]) / 2, y: (rect[1] + rect[3]) / 2 };
    let (hmon, mon_rect) = monitor_at(center);
    let fx_target = match target {
        RecordTarget::Window { hwnd } => fx_record::Target::Window { hwnd },
        RecordTarget::Region { left, top, right, bottom }
            if (left, top, right, bottom) != (mon_rect.left, mon_rect.top, mon_rect.right, mon_rect.bottom) =>
        {
            fx_record::Target::Region { hmonitor: hmon.0 as isize, left, top, right, bottom }
        }
        _ => fx_record::Target::Monitor { hmonitor: hmon.0 as isize },
    };

    // On cache la surcouche (et la barre) de la capture AVANT de démarrer, puis on laisse au
    // thread de rendu le temps d'appliquer l'exclusion.
    shared.modes.recording.store(true, Ordering::Release);
    shared.wake();
    std::thread::sleep(Duration::from_millis(120));

    let opts = fx_record::Options {
        target: fx_target,
        fps: cfg.record.fps,
        system_audio: cfg.record.system_audio,
        mic: cfg.record.mic,
        folder: videos_dir(shared),
    };
    match fx_record::Recording::start(opts) {
        Ok(rec) => {
            *RECORDING.lock().unwrap() = Some(rec);
            shared.fx.lock().unwrap().rec = Some((Instant::now(), rect));
            shared.wake();
        }
        Err(e) => {
            shared.modes.recording.store(false, Ordering::Release);
            shared.banner(format!("Enregistrement impossible : {e}"));
        }
    }
}

pub fn stop(shared: &Arc<Shared>) {
    let Some(rec) = RECORDING.lock().unwrap().take() else {
        // Arrêt pendant le compte à rebours : on annule.
        if shared.fx.lock().unwrap().countdown.take().is_some() {
            shared.banner("Enregistrement annulé");
        }
        return;
    };
    let shared = shared.clone();
    // La finalisation du MP4 peut prendre un instant : hors du thread moteur.
    std::thread::spawn(move || {
        let secs = rec.elapsed().as_secs();
        let result = rec.stop();
        shared.fx.lock().unwrap().rec = None;
        shared.modes.recording.store(false, Ordering::Release);
        match result {
            Ok((dir, warnings)) => {
                let mut msg = format!("Enregistrement terminé ({}:{:02})", secs / 60, secs % 60);
                if let Some(w) = warnings.first() {
                    msg.push_str(&format!(" · {w}"));
                }
                shared.banner(msg);
                if shared.config.load().record.open_folder_after {
                    let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
                }
            }
            Err(e) => shared.banner(format!("Enregistrement incomplet : {e}")),
        }
    });
}

pub fn pick(shared: &Shared, kind: u8) {
    if is_recording() {
        return;
    }
    shared.fx.lock().unwrap().select_rect = None;
    shared.modes.select.store(kind, Ordering::Release);
    shared.banner(if kind == 1 {
        "Clique sur la fenêtre à enregistrer · Échap annule"
    } else {
        "Trace la zone à enregistrer · Échap annule"
    });
}

fn cancel_pick(shared: &Shared) {
    shared.modes.select.store(0, Ordering::Release);
    shared.fx.lock().unwrap().select_rect = None;
    shared.banner("Enregistrement annulé");
}

/// Événements souris/clavier pendant le choix de la source.
pub fn on_select_mouse(shared: &Shared, button: MouseButton, x: i32, y: i32, down: bool) {
    let kind = shared.modes.select.load(Ordering::Acquire);
    if button == MouseButton::Right {
        if down {
            cancel_pick(shared);
        }
        return;
    }
    if button != MouseButton::Left {
        return;
    }
    match (kind, down) {
        (1, true) => {
            if let Some(hwnd) = window_at(POINT { x, y }) {
                shared.modes.select.store(0, Ordering::Release);
                fx_bus::action(Action::RecordStart(RecordTarget::Window { hwnd: hwnd.0 as isize }));
            }
        }
        (2, true) => {
            shared.fx.lock().unwrap().select_rect = Some([x, y, x, y]);
            shared.wake();
        }
        (2, false) => {
            let r = shared.fx.lock().unwrap().select_rect.take();
            shared.modes.select.store(0, Ordering::Release);
            if let Some([a, b, c, d]) = r {
                let (l, t, rr, bb) = (a.min(c), b.min(d), a.max(c), b.max(d));
                if rr - l >= 40 && bb - t >= 40 {
                    fx_bus::action(Action::RecordStart(RecordTarget::Region { left: l, top: t, right: rr, bottom: bb }));
                    return;
                }
            }
            shared.banner("Zone trop petite : enregistrement annulé");
        }
        _ => {}
    }
}

pub fn on_select_move(shared: &Shared, x: i32, y: i32) {
    let mut fx = shared.fx.lock().unwrap();
    if let Some(r) = fx.select_rect.as_mut() {
        r[2] = x;
        r[3] = y;
        drop(fx);
        shared.wake();
    }
}

pub fn on_select_key(shared: &Shared, vk: u32) {
    if vk == 0x1B {
        cancel_pick(shared);
    }
}
