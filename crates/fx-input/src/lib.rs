//! Hooks bas niveau, sur un thread dédié qui ne fait que pomper ses messages.
//!
//! Règle d'or : le callback ne fait presque rien (horodatage, publication sur le bus,
//! réveil du rendu). Windows retire silencieusement un hook trop lent.

use fx_bus::{Action, EventKind, MouseButton};
use fx_core::{Hotkey, Shared};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

struct Ctx {
    shared: Arc<Shared>,
}

static CTX: OnceLock<Ctx> = OnceLock::new();
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
static LAST_MOVE_PUBLISH_US: AtomicU64 = AtomicU64::new(0);

// État des modificateurs, suivi à partir des événements du hook lui-même.
static CTRL: AtomicBool = AtomicBool::new(false);
static ALT: AtomicBool = AtomicBool::new(false);
static SHIFT: AtomicBool = AtomicBool::new(false);
static WIN: AtomicBool = AtomicBool::new(false);
/// Touche principale d'un raccourci avalée à l'appui : on avale aussi son relâchement.
static SWALLOWED_VK: AtomicU32 = AtomicU32::new(0);

/// Démarre le thread des hooks. Appeler [`stop`] pour l'arrêter.
pub fn start(shared: Arc<Shared>) -> std::thread::JoinHandle<()> {
    let _ = CTX.set(Ctx { shared });
    std::thread::Builder::new()
        .name("clicksy-input".into())
        .spawn(|| unsafe { hook_thread() })
        .expect("thread des hooks")
}

pub fn stop() {
    let id = THREAD_ID.load(Ordering::Acquire);
    if id != 0 {
        unsafe {
            let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe fn hook_thread() {
    THREAD_ID.store(GetCurrentThreadId(), Ordering::Release);
    let hinst = GetModuleHandleW(None).ok().map(|m| m.into());
    let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), hinst, 0);
    let kbd = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), hinst, 0);
    if mouse.is_err() || kbd.is_err() {
        eprintln!("Clicksy : impossible d'installer les hooks ({mouse:?}, {kbd:?})");
    }

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    if let Ok(h) = mouse {
        let _ = UnhookWindowsHookEx(h);
    }
    if let Ok(h) = kbd {
        let _ = UnhookWindowsHookEx(h);
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        if let Some(ctx) = CTX.get() {
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let (x, y) = (info.pt.x, info.pt.y);
            let msg = wparam.0 as u32;
            let drawing = ctx.shared.modes.draw.load(Ordering::Relaxed);
            match msg {
                WM_MOUSEMOVE => {
                    ctx.shared.wake();
                    // Le bus reçoit un échantillon toutes les 8 ms au plus, sauf en mode dessin
                    // (chaque point compte pour un trait fluide). Le mouvement n'est jamais bloqué.
                    let now = fx_bus::now_us();
                    if drawing || now.saturating_sub(LAST_MOVE_PUBLISH_US.load(Ordering::Relaxed)) >= 8_000 {
                        LAST_MOVE_PUBLISH_US.store(now, Ordering::Relaxed);
                        fx_bus::publish(EventKind::MouseMove { x, y });
                    }
                }
                WM_LBUTTONDOWN => fx_bus::publish(EventKind::MouseDown { button: MouseButton::Left, x, y }),
                WM_LBUTTONUP => fx_bus::publish(EventKind::MouseUp { button: MouseButton::Left, x, y }),
                WM_RBUTTONDOWN => fx_bus::publish(EventKind::MouseDown { button: MouseButton::Right, x, y }),
                WM_RBUTTONUP => fx_bus::publish(EventKind::MouseUp { button: MouseButton::Right, x, y }),
                WM_MBUTTONDOWN => fx_bus::publish(EventKind::MouseDown { button: MouseButton::Middle, x, y }),
                WM_MBUTTONUP => fx_bus::publish(EventKind::MouseUp { button: MouseButton::Middle, x, y }),
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let button = if (info.mouseData >> 16) as u16 == 1 { MouseButton::X1 } else { MouseButton::X2 };
                    if msg == WM_XBUTTONDOWN {
                        fx_bus::publish(EventKind::MouseDown { button, x, y });
                    } else {
                        fx_bus::publish(EventKind::MouseUp { button, x, y });
                    }
                }
                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                    let delta = (info.mouseData >> 16) as i16 as i32;
                    fx_bus::publish(EventKind::Wheel { delta, horizontal: msg == WM_MOUSEHWHEEL, x, y });
                }
                _ => {}
            }
            // Mode dessin : boutons et molette servent à dessiner, l'app en dessous ne les reçoit pas.
            // Sauf sur la barre d'outils de Clicksy, qui doit rester cliquable.
            if drawing && msg != WM_MOUSEMOVE && !ctx.shared.modes.on_toolbar(x, y) {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        if let Some(ctx) = CTX.get() {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let vk = info.vkCode;
            let injected = info.flags.0 & LLKHF_INJECTED.0 != 0;
            let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);

            match VIRTUAL_KEY(vk as u16) {
                VK_LCONTROL | VK_RCONTROL | VK_CONTROL => CTRL.store(down, Ordering::Relaxed),
                VK_LMENU | VK_RMENU | VK_MENU => ALT.store(down, Ordering::Relaxed),
                VK_LSHIFT | VK_RSHIFT | VK_SHIFT => SHIFT.store(down, Ordering::Relaxed),
                VK_LWIN | VK_RWIN => WIN.store(down, Ordering::Relaxed),
                _ => {}
            }

            let scan = info.scanCode;
            fx_bus::publish(if down {
                EventKind::KeyDown { vk, scan, injected }
            } else {
                EventKind::KeyUp { vk, scan, injected }
            });

            // Raccourcis de Clicksy : on avale la touche pour ne pas gêner l'app active.
            if !injected {
                if down {
                    if let Some(action) = match_hotkey(ctx, vk) {
                        SWALLOWED_VK.store(vk, Ordering::Relaxed);
                        fx_bus::action(action);
                        return LRESULT(1);
                    }
                } else if SWALLOWED_VK.load(Ordering::Relaxed) == vk {
                    SWALLOWED_VK.store(0, Ordering::Relaxed);
                    return LRESULT(1);
                }
                // Mode dessin : les touches d'outils ne partent pas dans l'app active.
                if ctx.shared.modes.draw.load(Ordering::Relaxed) && is_draw_key(vk) {
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Touches utilisées en mode dessin : 1/2/3 couleurs, P/R/E/F outils, Z (Ctrl+Z), Échap, Suppr.
fn is_draw_key(vk: u32) -> bool {
    matches!(vk, 0x31..=0x33 | 0x50 | 0x52 | 0x45 | 0x46 | 0x5A | 0x1B | 0x2E)
}

fn match_hotkey(ctx: &Ctx, vk: u32) -> Option<Action> {
    let pressed = Hotkey {
        ctrl: CTRL.load(Ordering::Relaxed),
        alt: ALT.load(Ordering::Relaxed),
        shift: SHIFT.load(Ordering::Relaxed),
        win: WIN.load(Ordering::Relaxed),
        vk,
    };
    let cfg = ctx.shared.config.load();
    let hk = &cfg.hotkeys;
    let table = [
        (hk.toggle_all, Action::ToggleAll),
        (hk.cycle_spotlight, Action::ToggleSpotlight),
        (hk.toggle_ring, Action::ToggleRing),
        (hk.toggle_clicks, Action::ToggleClicks),
        (hk.toggle_draw, Action::ToggleDraw),
        (hk.toggle_laser, Action::ToggleLaser),
        (hk.toggle_trail, Action::ToggleTrail),
        (hk.toggle_toolbar, Action::ToggleToolbar),
        (hk.cycle_board, Action::CycleBoard),
        (hk.screenshot, Action::Screenshot),
    ];
    table.into_iter().find(|(h, _)| *h == Some(pressed)).map(|(_, a)| a)
}
