//! Thread de rendu : possède les fenêtres de surcouche (une par écran) et le GPU.
//!
//! Il dort tant que rien ne change (`MsgWaitForMultipleObjects` sur un événement de
//! réveil) : 0 % de CPU au repos. Il ne redessine que les écrans dont le contenu dépend
//! du curseur ou d'une animation, au plus une fois par rafraîchissement (`DwmFlush`).

mod annot;
mod draw;
mod gpu;
mod toolbar;

use fx_core::{Config, Shape, Shared};
use std::collections::VecDeque;
use gpu::{Gpu, Shown, Surface, CLASS_NAME};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;

static THREAD_ID: AtomicU32 = AtomicU32::new(0);
static REBUILD: AtomicBool = AtomicBool::new(false);
static RAISE: AtomicBool = AtomicBool::new(false);

/// Poignée d'événement Win32 partageable entre threads.
#[derive(Clone, Copy)]
struct WakeEvent(isize);
unsafe impl Send for WakeEvent {}
unsafe impl Sync for WakeEvent {}

pub fn start(shared: Arc<Shared>) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("clicksy-render".into())
        .spawn(move || unsafe { render_thread(shared) })
        .expect("thread de rendu")
}

pub fn stop() {
    let id = THREAD_ID.load(Ordering::Acquire);
    if id != 0 {
        unsafe {
            let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe fn render_thread(shared: Arc<Shared>) {
    THREAD_ID.store(GetCurrentThreadId(), Ordering::Release);

    let wake = CreateEventW(None, false, true, None).expect("événement de réveil");
    let we = WakeEvent(wake.0 as isize);
    shared.set_waker(move || {
        let _ = SetEvent(HANDLE(we.0 as *mut _));
    });

    register_class();
    toolbar::register(shared.clone());
    // Remet la surcouche au premier plan à chaque changement de fenêtre active
    // (diaporama PowerPoint, applis qui se mettent elles-mêmes « topmost »...).
    let foreground_hook = SetWinEventHook(
        EVENT_SYSTEM_FOREGROUND,
        EVENT_SYSTEM_FOREGROUND,
        None,
        Some(on_foreground),
        0,
        0,
        WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
    );

    let mut gpu: Option<Gpu> = None;
    let mut surfaces: Vec<Surface> = Vec::new();
    let mut needs_rebuild = true;
    let mut last_raise = Instant::now();
    let mut rstate = RenderState::default();

    'main: loop {
        // 1. Messages Windows (changement d'écran, de DPI, premier plan...).
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                break 'main;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if REBUILD.swap(false, Ordering::AcqRel) {
            needs_rebuild = true;
        }

        // 2. (Re)création du GPU et des surfaces si nécessaire.
        if needs_rebuild {
            toolbar::destroy(&shared);
            surfaces.clear();
            if gpu.is_none() {
                gpu = Gpu::new().map_err(|e| eprintln!("Clicksy : GPU indisponible : {e}")).ok();
            }
            if let Some(g) = &gpu {
                surfaces = create_surfaces(g);
            }
            needs_rebuild = false;
            if gpu.is_none() {
                // Nouvel essai dans une seconde.
                std::thread::sleep(Duration::from_secs(1));
                needs_rebuild = true;
                continue;
            }
        }
        if RAISE.swap(false, Ordering::AcqRel) || last_raise.elapsed() > Duration::from_secs(2) {
            surfaces.iter().for_each(Surface::raise);
            toolbar::raise();
            last_raise = Instant::now();
        }

        // 3. Rendu.
        let Some(g) = &gpu else { continue };
        match render_frame(g, &mut surfaces, &shared, &mut rstate) {
            Ok((presented, animating)) => {
                if presented {
                    // Cadence calée sur le compositeur : jamais plus d'une image par rafraîchissement.
                    let _ = DwmFlush();
                }
                if animating {
                    continue;
                }
            }
            Err(e) => {
                // Périphérique perdu (pilote mis à jour, veille...) : on reconstruit tout.
                eprintln!("Clicksy : erreur de rendu, reconstruction : {e}");
                toolbar::destroy(&shared);
                surfaces.clear();
                gpu = None;
                needs_rebuild = true;
                continue;
            }
        }

        // 4. Sommeil jusqu'au prochain réveil (souris, config) ou message.
        // Délai de 2 s pour réaffirmer périodiquement le premier plan.
        // Pendant un enregistrement, réveil 4 fois par seconde pour le chrono de la barre.
        let timeout = if shared.modes.recording.load(Ordering::Acquire) { 250 } else { 2000 };
        let _ = MsgWaitForMultipleObjects(Some(&[wake]), false, timeout, QS_ALLINPUT);
    }

    toolbar::destroy(&shared);
    surfaces.clear();
    if !foreground_hook.is_invalid() {
        let _ = UnhookWinEvent(foreground_hook);
    }
    let _ = CloseHandle(wake);
}

/// État propre au thread de rendu, conservé d'une image à l'autre.
#[derive(Default)]
struct RenderState {
    /// Positions récentes du curseur (x, y, instant), pour la traînée.
    trail: VecDeque<(f32, f32, Instant)>,
    /// Copie des formes dessinées, mise à jour seulement quand elles changent.
    shapes: Vec<Shape>,
    shapes_gen: u64,
}

const BANNER_MS: f32 = 1800.0;

/// Dessine une image sur les écrans qui en ont besoin.
/// Renvoie (au moins un écran présenté, une animation est en cours).
unsafe fn render_frame(
    gpu: &Gpu,
    surfaces: &mut [Surface],
    shared: &Shared,
    st: &mut RenderState,
) -> windows::core::Result<(bool, bool)> {
    let cfg_guard = shared.config.load();
    let cfg: &Config = &cfg_guard;
    let now = Instant::now();
    let draw_mode = shared.modes.draw.load(Ordering::Acquire);
    let laser = shared.modes.laser.load(Ordering::Acquire);
    let board = shared.modes.board.load(Ordering::Acquire);
    let board_pt = (shared.modes.board_x.load(Ordering::Acquire), shared.modes.board_y.load(Ordering::Acquire));
    let select = shared.modes.select.load(Ordering::Acquire);
    let recording = shared.modes.recording.load(Ordering::Acquire);
    // Enregistrement en mode « éditable » : la surcouche n'apparaît pas dans la vidéo.
    let exclude = recording && shared.modes.rec_exclude.load(Ordering::Acquire);
    for s in surfaces.iter_mut() {
        s.set_excluded(exclude);
    }

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let scale = surfaces.iter().find(|s| s.contains(pt.x, pt.y)).map(|s| s.scale).unwrap_or(1.0);

    let (ripples, held, draw_tool, draw_color, banner, color_index, draw_width, zone, countdown, rec) = {
        let mut fx = shared.fx.lock().unwrap();
        let dur = Duration::from_millis(cfg.clicks.duration_ms as u64);
        fx.ripples.retain(|r| now.saturating_duration_since(r.start) < dur);
        if fx.banner.as_ref().is_some_and(|b| now.saturating_duration_since(b.start).as_secs_f32() * 1000.0 > BANNER_MS) {
            fx.banner = None;
        }
        let d = &fx.draw;
        if d.generation != st.shapes_gen {
            st.shapes = d.shapes.iter().chain(d.current.iter()).cloned().collect();
            st.shapes_gen = d.generation;
        }
        let color = cfg.draw.colors[d.color_index.min(2)];
        let banner = fx.banner.as_ref().map(|b| (b.text.clone(), now.saturating_duration_since(b.start).as_secs_f32() * 1000.0));
        let countdown = fx.countdown.map(|(start, r)| {
            let left = 3.0 - now.saturating_duration_since(start).as_secs_f32();
            (left.ceil().max(1.0) as u32, r)
        });
        (fx.ripples.clone(), fx.held, d.tool, color, banner, d.color_index, d.width, fx.select_rect, countdown, fx.rec)

    };
    // Fondu d'entrée (120 ms) et de sortie (400 ms) de la bulle.
    let banner = banner.map(|(t, ms)| {
        let a = (ms / 120.0).min(1.0).min(((BANNER_MS - ms) / 400.0).clamp(0.0, 1.0));
        (t, a)
    });

    // Traînée : on enregistre la position à chaque image, et on oublie les points trop vieux.
    let trail_on = laser || (cfg.enabled && cfg.trail.enabled);
    let trail_ms = if laser { cfg.laser.duration_ms } else { cfg.trail.duration_ms } as f32;
    if trail_on {
        let p = (pt.x as f32, pt.y as f32);
        if st.trail.back().is_none_or(|b| (b.0 - p.0).abs() + (b.1 - p.1).abs() >= 1.0) {
            st.trail.push_back((p.0, p.1, now));
        }
        while st.trail.front().is_some_and(|f| now.saturating_duration_since(f.2).as_secs_f32() * 1000.0 > trail_ms) {
            st.trail.pop_front();
        }
    } else {
        st.trail.clear();
    }
    let trail: Vec<(f32, f32, f32)> = st
        .trail
        .iter()
        .map(|(x, y, t)| (*x, *y, 1.0 - now.saturating_duration_since(*t).as_secs_f32() * 1000.0 / trail_ms))
        .collect();

    let pulse = if cfg.ring.pulse {
        let period = cfg.ring.pulse_period_ms.max(1) as u128;
        (now_ms() % period) as f32 / period as f32
    } else {
        0.0
    };

    let frame = draw::Frame {
        cfg,
        cursor: (pt.x, pt.y),
        scale,
        now,
        ripples: &ripples,
        held,
        pulse,
        shapes: &st.shapes,
        draw_mode,
        draw_tool,
        draw_color,
        laser,
        trail: &trail,
        banner: banner.as_ref().map(|(t, a)| (t.as_str(), *a)),
        board,
        board_pt,
        select,
        select_rect: if select == 1 { hovered_window(pt) } else { zone },
        countdown,
        rec_rect: rec.map(|r| r.1).filter(|r| r[2] > r[0]),
    };

    // Le contenu « statique » dépend de la config et des formes dessinées.
    let token = (Arc::as_ptr(&cfg_guard) as usize).wrapping_mul(31).wrapping_add(st.shapes_gen as usize).wrapping_add((board as usize) << 40).wrapping_add((rec.is_some() as usize) << 44);
    let mut presented = false;
    for s in surfaces.iter_mut() {
        let dynamic = draw::is_dynamic(s, &frame);
        let want = if dynamic { Shown::Dynamic } else { Shown::Static(token) };
        // Contenu statique déjà affiché : rien à faire.
        if !dynamic && s.shown == want {
            continue;
        }
        draw::draw(gpu, s, &frame)?;
        s.shown = want;
        presented = true;
    }

    // Barre d'outils (créée, mise à jour ou masquée selon la config).
    let view = toolbar::View {
        draw_mode,
        tool: draw_tool,
        color_index,
        width: draw_width,
        laser,
        board,
        recording,
        rec_secs: rec.map(|r| now.saturating_duration_since(r.0).as_secs()),
        countdown: countdown.is_some(),
    };
    presented |= toolbar::sync(gpu, shared, cfg, &view)?;

    let animating = countdown.is_some()
        || (cfg.enabled && (!ripples.is_empty() || (cfg.ring.enabled && cfg.ring.pulse && !laser && !draw_mode)))
        || trail.len() >= 2
        || banner.is_some();
    Ok((presented, animating))
}

fn now_ms() -> u128 {
    use std::sync::OnceLock;
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_millis()
}

unsafe fn create_surfaces(gpu: &Gpu) -> Vec<Surface> {
    let mut monitors: Vec<(RECT, f32)> = Vec::new();
    let _ = EnumDisplayMonitors(None, None, Some(enum_monitor), LPARAM(&mut monitors as *mut _ as isize));
    monitors
        .into_iter()
        .filter_map(|(rect, scale)| {
            Surface::new(gpu, rect, scale)
                .map_err(|e| eprintln!("Clicksy : surface impossible pour {rect:?} : {e}"))
                .ok()
        })
        .collect()
}

unsafe extern "system" fn enum_monitor(hmon: HMONITOR, _hdc: HDC, _rc: *mut RECT, data: LPARAM) -> windows::core::BOOL {
    let list = &mut *(data.0 as *mut Vec<(RECT, f32)>);
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(hmon, &mut info).as_bool() {
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        list.push((info.rcMonitor, dx as f32 / 96.0));
    }
    true.into()
}

unsafe fn register_class() {
    let hinst = GetModuleHandleW(None).unwrap_or_default();
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinst.into(),
        lpszClassName: CLASS_NAME,
        hCursor: HCURSOR::default(),
        ..Default::default()
    };
    RegisterClassExW(&wc);
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Transparence aux clics : la souris « traverse » la surcouche.
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            REBUILD.store(true, Ordering::Release);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    RAISE.store(true, Ordering::Release);
}

/// Fenêtre principale sous le curseur (hors Clicksy), pour le choix « fenêtre ».
fn hovered_window(pt: POINT) -> Option<[i32; 4]> {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
    unsafe {
        let hwnd = GetAncestor(WindowFromPoint(pt), GA_ROOT);
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            return None;
        }
        let mut r = RECT::default();
        DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).ok()?;
        Some([r.left, r.top, r.right, r.bottom])
    }
}
