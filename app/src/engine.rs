//! Moteur : consomme le bus d'événements et fait évoluer l'état.
//! Seul endroit (avec les réglages) où la configuration et les dessins changent.

use crossbeam_channel::Receiver;
use fx_bus::{Action, Event, EventKind, MouseButton};
use fx_core::{paths, Config, DrawTool, Ripple, Shape, Shared};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

const VK_SHIFTS: [u32; 3] = [0x10, 0xA0, 0xA1];
const VK_CTRLS: [u32; 3] = [0x11, 0xA2, 0xA3];

#[derive(Default)]
struct Keys {
    shift: bool,
    ctrl: bool,
}

pub fn start(shared: Arc<Shared>, rx: Receiver<Event>, on_config_changed: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("clicksy-engine".into())
        .spawn(move || {
            let mut keys = Keys::default();
            for ev in rx {
                let drawing = shared.modes.draw.load(Ordering::Relaxed);
                match ev.kind {
                    EventKind::KeyDown { vk, injected, .. } | EventKind::KeyUp { vk, injected, .. } if !injected => {
                        let down = matches!(ev.kind, EventKind::KeyDown { .. });
                        if VK_SHIFTS.contains(&vk) {
                            keys.shift = down;
                            if drawing {
                                update_current(&shared, None, keys.shift);
                            }
                        } else if VK_CTRLS.contains(&vk) {
                            keys.ctrl = down;
                        } else if down && drawing && draw_key(&shared, vk, &keys) {
                            on_config_changed();
                        }
                    }
                    EventKind::MouseDown { button, x, y } if drawing => draw_mouse(&shared, button, x, y, true, &keys),
                    EventKind::MouseUp { button, x, y } if drawing => draw_mouse(&shared, button, x, y, false, &keys),
                    EventKind::MouseMove { x, y } if drawing => update_current(&shared, Some((x as f32, y as f32)), keys.shift),
                    EventKind::Wheel { delta, .. } if drawing => {
                        let mut fx = shared.fx.lock().unwrap();
                        let step = if delta > 0 { 1.0 } else { -1.0 };
                        fx.draw.width = (fx.draw.width + step).clamp(1.0, 40.0);
                        let w = fx.draw.width;
                        drop(fx);
                        shared.banner(format!("Épaisseur : {w:.0}"));
                    }
                    EventKind::MouseDown { button, x, y } => on_mouse(&shared, button, x, y, true),
                    EventKind::MouseUp { button, x, y } => on_mouse(&shared, button, x, y, false),
                    EventKind::Action(a) => {
                        if apply(&shared, a) {
                            on_config_changed();
                        }
                    }
                    _ => {}
                }
            }
        })
        .expect("thread moteur");
}

fn button_index(b: MouseButton) -> Option<usize> {
    match b {
        MouseButton::Left => Some(0),
        MouseButton::Right => Some(1),
        MouseButton::Middle => Some(2),
        _ => None,
    }
}

fn on_mouse(shared: &Shared, button: MouseButton, x: i32, y: i32, down: bool) {
    let Some(i) = button_index(button) else { return };
    let cfg = shared.config.load();
    {
        let mut fx = shared.fx.lock().unwrap();
        fx.held[i] = down;
        if down && cfg.enabled && cfg.clicks.enabled {
            fx.ripples.push(Ripple { x, y, button, start: Instant::now() });
            // Garde-fou si quelqu'un mitraille : on ne garde que les 32 dernières ondes.
            let excess = fx.ripples.len().saturating_sub(32);
            fx.ripples.drain(..excess);
        }
    }
    shared.wake();
    if down && cfg.enabled && cfg.clicks.enabled && cfg.clicks.sound {
        fx_audio::play_click(cfg.clicks.sound_volume);
    }
}

// ---------------------------------------------------------------- Dessin

fn next_tool(t: DrawTool) -> DrawTool {
    match t {
        DrawTool::Pen => DrawTool::Rect,
        DrawTool::Rect => DrawTool::Ellipse,
        DrawTool::Ellipse => DrawTool::Arrow,
        DrawTool::Arrow => DrawTool::Pen,
    }
}

fn draw_mouse(shared: &Shared, button: MouseButton, x: i32, y: i32, down: bool, keys: &Keys) {
    let cfg = shared.config.load();
    let p = (x as f32, y as f32);
    let mut fx = shared.fx.lock().unwrap();
    let d = &mut fx.draw;
    match (button, down) {
        (MouseButton::Left, true) => {
            d.current = Some(Shape {
                tool: d.tool,
                color: cfg.draw.colors[d.color_index.min(2)],
                width: d.width,
                points: vec![p, p],
                constrained: keys.shift,
            });
        }
        (MouseButton::Left, false) => {
            if let Some(mut s) = d.current.take() {
                if s.tool != DrawTool::Pen {
                    s.points[1] = p;
                }
                if !is_tiny(&s) {
                    d.shapes.push(s);
                }
            }
        }
        // Clic droit : outil suivant.
        (MouseButton::Right, true) => {
            d.tool = next_tool(d.tool);
            let label = d.tool.label_fr();
            d.generation += 1;
            drop(fx);
            shared.banner(format!("Outil : {label}"));
            return;
        }
        _ => return,
    }
    d.generation += 1;
    drop(fx);
    shared.wake();
}

fn is_tiny(s: &Shape) -> bool {
    let (a, b) = (s.points[0], *s.points.last().unwrap());
    match s.tool {
        DrawTool::Pen => s.points.len() < 3 && (a.0 - b.0).abs() + (a.1 - b.1).abs() < 2.0,
        _ => (a.0 - b.0).abs() < 3.0 && (a.1 - b.1).abs() < 3.0,
    }
}

/// Met à jour la forme en cours (nouveau point, ou simple changement de Maj).
fn update_current(shared: &Shared, p: Option<(f32, f32)>, shift: bool) {
    let mut fx = shared.fx.lock().unwrap();
    let d = &mut fx.draw;
    let Some(s) = d.current.as_mut() else { return };
    s.constrained = shift;
    if let Some(p) = p {
        if s.tool == DrawTool::Pen {
            let last = *s.points.last().unwrap();
            // On ignore les micro-déplacements : trait plus propre et moins de points.
            if (p.0 - last.0).powi(2) + (p.1 - last.1).powi(2) >= 2.0 {
                s.points.push(p);
            }
        } else {
            s.points[1] = p;
        }
    }
    d.generation += 1;
    drop(fx);
    shared.wake();
}

/// Touches du mode dessin. Renvoie `true` si le mode dessin a été quitté.
fn draw_key(shared: &Shared, vk: u32, keys: &Keys) -> bool {
    let cfg = shared.config.load();
    let mut fx = shared.fx.lock().unwrap();
    let d = &mut fx.draw;
    let mut banner: Option<String> = None;
    let mut exit = false;
    match vk {
        0x31..=0x33 => {
            d.color_index = (vk - 0x31) as usize;
            banner = Some(format!("Couleur {}", d.color_index + 1));
        }
        0x50 => d.tool = DrawTool::Pen,
        0x52 => d.tool = DrawTool::Rect,
        0x45 => d.tool = DrawTool::Ellipse,
        0x46 => d.tool = DrawTool::Arrow,
        0x5A if keys.ctrl => {
            d.current = None;
            if d.shapes.pop().is_some() {
                banner = Some("Annulé".into());
            }
        }
        0x2E => {
            d.shapes.clear();
            d.current = None;
            banner = Some("Dessins effacés".into());
        }
        0x1B => {
            // Échap : efface tout ; s'il n'y a rien à effacer, quitte le mode dessin.
            if d.shapes.is_empty() && d.current.is_none() {
                exit = true;
            } else {
                d.shapes.clear();
                d.current = None;
                banner = Some("Dessins effacés".into());
            }
        }
        _ => return false,
    }
    if matches!(vk, 0x50 | 0x52 | 0x45 | 0x46) {
        banner = Some(format!("Outil : {}", d.tool.label_fr()));
    }
    d.generation += 1;
    drop(fx);
    drop(cfg);
    if exit {
        set_draw_mode(shared, false);
        return true;
    }
    match banner {
        Some(b) => shared.banner(b),
        None => shared.wake(),
    }
    false
}

fn set_draw_mode(shared: &Shared, on: bool) {
    let cfg = shared.config.load();
    shared.modes.draw.store(on, Ordering::Release);
    {
        let mut fx = shared.fx.lock().unwrap();
        let d = &mut fx.draw;
        d.current = None;
        if on {
            d.tool = cfg.draw.default_tool;
            d.width = cfg.draw.width;
        } else if cfg.draw.clear_on_exit {
            d.shapes.clear();
        }
        d.generation += 1;
        fx.held = [false; 3];
    }
    if on {
        shared.banner("Mode dessin · 1 2 3 couleurs · P stylo · R cadre · E rond · F flèche · Échap efface");
    } else {
        shared.banner("Mode dessin terminé");
    }
}

// ---------------------------------------------------------------- Actions

fn on_off(b: bool) -> &'static str {
    if b { "activé" } else { "désactivé" }
}

/// Applique une action. Renvoie `true` si la configuration ou un mode a changé.
fn apply(shared: &Shared, action: Action) -> bool {
    match action {
        Action::Quit => return false,
        Action::ReloadConfig => {
            let (cfg, _warn) = paths::load_config(&shared.data_dir);
            shared.config.store(Arc::new(cfg));
            shared.wake();
            return true;
        }
        Action::ToggleDraw => {
            let on = !shared.modes.draw.load(Ordering::Acquire);
            set_draw_mode(shared, on);
            return true;
        }
        Action::ToggleLaser => {
            let on = !shared.modes.laser.load(Ordering::Acquire);
            shared.modes.laser.store(on, Ordering::Release);
            shared.banner(format!("Pointeur laser {}", on_off(on)));
            return true;
        }
        ref a => shared.update_config(|c| match a {
            Action::ToggleAll => c.enabled = !c.enabled,
            Action::CycleSpotlight => {
                c.spotlight.mode = c.spotlight.mode.next();
                c.enabled = true;
            }
            Action::SetSpotlight(m) => c.spotlight.mode = *m,
            Action::ToggleRing => c.ring.enabled = !c.ring.enabled,
            Action::TogglePulse => c.ring.pulse = !c.ring.pulse,
            Action::ToggleClicks => c.clicks.enabled = !c.clicks.enabled,
            Action::ToggleClickSound => c.clicks.sound = !c.clicks.sound,
            Action::ToggleTrail => c.trail.enabled = !c.trail.enabled,
            _ => {}
        }),
    }
    let cfg = shared.config.load_full();
    let _ = paths::save_config(&shared.data_dir, &cfg);
    if let Some(text) = banner_for(&action, &cfg) {
        shared.banner(text);
    }
    true
}

fn banner_for(action: &Action, c: &Config) -> Option<String> {
    Some(match action {
        Action::ToggleAll => if c.enabled { "Clicksy : effets activés".into() } else { "Clicksy : effets en pause".into() },
        Action::CycleSpotlight | Action::SetSpotlight(_) => format!("Spotlight : {}", c.spotlight.mode.label_fr().to_lowercase()),
        Action::ToggleRing => format!("Anneau {}", on_off(c.ring.enabled)),
        Action::TogglePulse => format!("Pulsation {}", if c.ring.pulse { "activée" } else { "désactivée" }),
        Action::ToggleClicks => format!("Effets de clic {}", on_off(c.clicks.enabled)),
        Action::ToggleClickSound => format!("Son de clic {}", on_off(c.clicks.sound)),
        Action::ToggleTrail => format!("Traînée {}", if c.trail.enabled { "activée" } else { "désactivée" }),
        _ => return None,
    })
}
