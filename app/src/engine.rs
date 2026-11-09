//! Moteur : consomme le bus d'événements et fait évoluer l'état.
//! Seul endroit (avec les réglages) où la configuration et les dessins changent.

use crossbeam_channel::Receiver;
use fx_bus::{Action, Event, EventKind, MouseButton};
use fx_core::{paths, t, tf, Config, DrawTool, Ripple, Shape, Shared};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

const VK_SHIFTS: [u32; 3] = [0x10, 0xA0, 0xA1];
const VK_CTRLS: [u32; 3] = [0x11, 0xA2, 0xA3];
const WIDTHS: [f32; 4] = [3.0, 5.0, 9.0, 14.0];

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
            let mut shown = crate::keys::Keys::default();
            for ev in rx {
                shown.on_event(&shared, &ev.kind);
                let drawing = shared.modes.draw.load(Ordering::Relaxed);
                // Choix de la source d'enregistrement : la souris sert à désigner fenêtre ou zone.
                if shared.modes.select.load(Ordering::Relaxed) != 0 {
                    match ev.kind {
                        EventKind::MouseDown { button, x, y } if !shared.modes.on_toolbar(x, y) => {
                            crate::recorder::on_select_mouse(&shared, button, x, y, true);
                            continue;
                        }
                        EventKind::MouseUp { button, x, y } if !shared.modes.on_toolbar(x, y) => {
                            crate::recorder::on_select_mouse(&shared, button, x, y, false);
                            continue;
                        }
                        EventKind::MouseMove { x, y } => {
                            crate::recorder::on_select_move(&shared, x, y);
                            continue;
                        }
                        EventKind::KeyDown { vk, injected: false, .. } => {
                            crate::recorder::on_select_key(&shared, vk);
                            continue;
                        }
                        _ => {}
                    }
                }
                // Les clics sur la barre d'outils lui appartiennent : ni onde, ni trait.
                if let EventKind::MouseDown { x, y, .. } | EventKind::MouseUp { x, y, .. } = ev.kind {
                    if shared.modes.on_toolbar(x, y) {
                        continue;
                    }
                }
                // Guide pas à pas : chaque clic (hors dessin) devient une étape.
                if let EventKind::MouseDown { button, x, y } = ev.kind {
                    if !drawing && shared.modes.guide.load(Ordering::Relaxed) {
                        crate::guide::on_click(&shared, button, x, y);
                    }
                }
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
                    EventKind::Wheel { delta, x, y, .. } if drawing && !shared.modes.on_toolbar(x, y) => {
                        let w = {
                            let mut fx = shared.fx.lock().unwrap();
                            let step = if delta > 0 { 1.0 } else { -1.0 };
                            fx.draw.width = (fx.draw.width + step).clamp(1.0, 40.0);
                            fx.draw.generation += 1;
                            fx.draw.width
                        };
                        shared.banner(tf("Épaisseur : {}", &[&format!("{w:.0}")]));
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

fn tool_from(i: u8) -> DrawTool {
    match i {
        1 => DrawTool::Rect,
        2 => DrawTool::Ellipse,
        3 => DrawTool::Arrow,
        _ => DrawTool::Pen,
    }
}

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
            shared.banner(tf("Outil : {}", &[t(label)]));
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

/// Modifie l'état de dessin puis affiche éventuellement une bulle.
fn with_draw(shared: &Shared, f: impl FnOnce(&mut fx_core::DrawState) -> Option<String>) {
    let banner = {
        let mut fx = shared.fx.lock().unwrap();
        let b = f(&mut fx.draw);
        fx.draw.generation += 1;
        b
    };
    match banner {
        Some(b) => shared.banner(b),
        None => shared.wake(),
    }
}

fn set_tool(shared: &Shared, tool: DrawTool) {
    if !shared.modes.draw.load(Ordering::Acquire) {
        set_draw_mode(shared, true, false);
    }
    with_draw(shared, |d| {
        d.tool = tool;
        Some(tf("Outil : {}", &[t(tool.label_fr())]))
    });
}

fn undo(shared: &Shared) {
    with_draw(shared, |d| {
        d.current = None;
        d.shapes.pop().map(|_| t("Annulé").to_string())
    });
}

fn clear(shared: &Shared) {
    // Tableau : on garde une image avant d'effacer (comme on photographie un vrai tableau).
    let saved = save_board(shared);
    with_draw(shared, |d| {
        d.current = None;
        d.shapes.clear();
        Some(t(if saved { "Tableau enregistré puis effacé" } else { "Dessins effacés" }).into())
    });
}

/// Génération des dessins lors de la dernière image du tableau (évite les doublons).
static BOARD_SAVED_GEN: AtomicU64 = AtomicU64::new(u64::MAX);

/// Enregistre une image du tableau ouvert s'il contient des dessins. Renvoie `true` si c'est fait.
fn save_board(shared: &Shared) -> bool {
    let m = &shared.modes;
    if !shared.config.load().draw.save_board || m.board.load(Ordering::Acquire) == 0 {
        return false;
    }
    let (empty, generation) = {
        let fx = shared.fx.lock().unwrap();
        (fx.draw.shapes.is_empty() && fx.draw.current.is_none(), fx.draw.generation)
    };
    if empty || BOARD_SAVED_GEN.load(Ordering::Acquire) == generation {
        return false;
    }
    let pt = POINT { x: m.board_x.load(Ordering::Acquire), y: m.board_y.load(Ordering::Acquire) };
    match crate::capture::save(shared, pt, t("Tableau"), false) {
        Ok(_) => {
            BOARD_SAVED_GEN.store(generation, Ordering::Release);
            true
        }
        Err(e) => {
            shared.banner(tf("Image du tableau impossible : {}", &[&e]));
            false
        }
    }
}

/// Touches du mode dessin. Renvoie `true` si le mode dessin a été quitté.
fn draw_key(shared: &Shared, vk: u32, keys: &Keys) -> bool {
    match vk {
        0x31..=0x33 => set_color(shared, (vk - 0x31) as usize),
        0x50 => set_tool(shared, DrawTool::Pen),
        0x52 => set_tool(shared, DrawTool::Rect),
        0x45 => set_tool(shared, DrawTool::Ellipse),
        0x46 => set_tool(shared, DrawTool::Arrow),
        0x5A if keys.ctrl => undo(shared),
        0x2E => clear(shared),
        0x1B => {
            // Échap : efface tout ; s'il n'y a rien à effacer, quitte le mode dessin.
            let empty = {
                let fx = shared.fx.lock().unwrap();
                fx.draw.shapes.is_empty() && fx.draw.current.is_none()
            };
            if empty {
                set_draw_mode(shared, false, true);
                return true;
            }
            clear(shared);
        }
        _ => {}
    }
    false
}

fn set_color(shared: &Shared, i: usize) {
    with_draw(shared, |d| {
        d.color_index = i.min(2);
        Some(tf("Couleur {}", &[&(d.color_index + 1).to_string()]))
    });
}

fn set_draw_mode(shared: &Shared, on: bool, banner: bool) {
    let saved = !on && save_board(shared);
    let cfg = shared.config.load();
    shared.modes.draw.store(on, Ordering::Release);
    {
        let mut fx = shared.fx.lock().unwrap();
        fx.held = [false; 3];
        let d = &mut fx.draw;
        d.current = None;
        if on {
            d.tool = cfg.draw.default_tool;
            d.width = cfg.draw.width;
        } else {
            // Quitter le dessin ferme aussi le tableau (sinon les clics iraient aux applis cachées).
            if shared.modes.board.swap(0, Ordering::AcqRel) != 0 {
                std::mem::swap(&mut d.shapes, &mut d.stash);
                d.stash.clear();
            }
            if cfg.draw.clear_on_exit {
                d.shapes.clear();
            }
        }
        d.generation += 1;
    }
    if banner {
        if on {
            shared.banner(t("Mode dessin · 1 2 3 couleurs · P stylo · R cadre · E rond · F flèche · Échap efface"));
        } else {
            shared.banner(t(if saved { "Souris normale · image du tableau enregistrée" } else { "Souris normale" }));
        }
    } else {
        shared.wake();
    }
}

/// Raccourci : désactivé → blanc → noir → désactivé.
fn cycle_board(shared: &Shared) {
    let cur = shared.modes.board.load(Ordering::Acquire);
    set_board(shared, (cur + 1) % 3);
}

/// Ouvre le tableau blanc (1) ou noir (2), ou le ferme (0). Redemander le tableau déjà ouvert
/// le ferme. Les dessins de l'écran sont mis de côté pendant que le tableau est ouvert.
fn set_board(shared: &Shared, target: u8) {
    let m = &shared.modes;
    let cur = m.board.load(Ordering::Acquire);
    let next = if target == cur { 0 } else { target };
    if cur == 0 && next != 0 {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        m.board_x.store(pt.x, Ordering::Release);
        m.board_y.store(pt.y, Ordering::Release);
        if !m.draw.load(Ordering::Acquire) {
            set_draw_mode(shared, true, false);
        }
        with_draw(shared, |d| {
            d.current = None;
            std::mem::swap(&mut d.shapes, &mut d.stash);
            None
        });
    }
    let mut saved = false;
    if cur != 0 && next == 0 {
        saved = save_board(shared);
        with_draw(shared, |d| {
            d.current = None;
            std::mem::swap(&mut d.shapes, &mut d.stash);
            d.stash.clear();
            None
        });
    }
    m.board.store(next, Ordering::Release);
    shared.banner(t(match next {
        1 => "Tableau blanc · dessine directement · Échap efface",
        2 => "Tableau noir",
        _ if saved => "Tableau fermé · image enregistrée",
        _ => "Tableau fermé",
    }));
}

// ---------------------------------------------------------------- Actions

fn on_off(b: bool) -> &'static str {
    fx_core::i18n::on_off(b)
}

/// Applique une action. Renvoie `true` si la configuration ou un mode a changé.
fn apply(shared: &Arc<Shared>, action: Action) -> bool {
    match action {
        Action::Quit => return false,
        Action::ReloadConfig => {
            let (cfg, _warn) = paths::load_config(&shared.data_dir);
            shared.update_config(|c| *c = cfg);
            shared.wake();
            return true;
        }
        Action::ToggleDraw => {
            let on = !shared.modes.draw.load(Ordering::Acquire);
            set_draw_mode(shared, on, true);
            return true;
        }
        Action::ExitDraw => {
            set_draw_mode(shared, false, true);
            return true;
        }
        Action::SetTool(i) => {
            set_tool(shared, tool_from(i));
            return true;
        }
        Action::SetColor(i) => {
            set_color(shared, i as usize);
            return true;
        }
        Action::CycleWidth => {
            with_draw(shared, |d| {
                let next = WIDTHS.iter().copied().find(|w| *w > d.width + 0.5).unwrap_or(WIDTHS[0]);
                d.width = next;
                Some(tf("Épaisseur : {}", &[&format!("{next:.0}")]))
            });
            return true;
        }
        Action::Undo => {
            undo(shared);
            return false;
        }
        Action::ClearDrawings => {
            clear(shared);
            return false;
        }
        Action::CycleBoard => {
            cycle_board(shared);
            return true;
        }
        Action::SetBoard(n) => {
            set_board(shared, n.min(2));
            return true;
        }
        Action::RecordStart(t) => {
            crate::recorder::start(shared, t);
            return false;
        }
        Action::RecordPick(k) => {
            crate::recorder::pick(shared, k);
            return false;
        }
        Action::RecordGo(t) => {
            crate::recorder::go(shared, t);
            return true;
        }
        Action::RecordStop => {
            crate::recorder::stop(shared);
            return true;
        }
        Action::ToggleRecord => {
            if crate::recorder::is_recording() || shared.fx.lock().unwrap().countdown.is_some() {
                crate::recorder::stop(shared);
            } else {
                crate::recorder::start(shared, fx_bus::RecordTarget::Screen);
            }
            return true;
        }
        Action::OpenVideosFolder | Action::OpenSessionFolder if crate::session::enabled(shared) => {
            crate::session::open(shared);
            return false;
        }
        Action::OpenVideosFolder | Action::OpenSessionFolder => {
            let dir = crate::recorder::videos_dir(shared);
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
            return false;
        }
        Action::ApplyProfile(i) => {
            apply_profile(shared, i as usize);
            return true;
        }
        Action::NextProfile => {
            let cfg = shared.config.load();
            let cur = cfg.profiles.iter().position(|p| p.name == cfg.active_profile);
            let next = cur.map_or(0, |i| (i + 1) % cfg.profiles.len().max(1));
            apply_profile(shared, next);
            return true;
        }
        Action::SetFormation(i) => {
            let name = shared.config.load().session.formations.get(i as usize).cloned().unwrap_or_default();
            crate::session::set_formation(shared, &name);
            return true;
        }
        Action::ToggleGuide => {
            crate::guide::toggle(shared);
            return true;
        }
        Action::OpenEditor => {
            crate::editor::open(None);
            return false;
        }
        Action::NewSession => {
            crate::session::new_session(shared);
            return false;
        }
        Action::Screenshot => {
            crate::capture::take(shared);
            return false;
        }
        Action::OpenSettings => {
            crate::settings::open();
            return false;
        }
        Action::ToggleLaser => {
            let on = !shared.modes.laser.load(Ordering::Acquire);
            shared.modes.laser.store(on, Ordering::Release);
            shared.banner(tf("Pointeur laser {}", &[on_off(on)]));
            return true;
        }
        ref a => shared.update_config(|c| match a {
            Action::ToggleAll => c.enabled = !c.enabled,
            Action::CycleSpotlight => {
                c.spotlight.mode = c.spotlight.mode.next();
                c.enabled = true;
            }
            Action::SetSpotlight(m) => c.spotlight.mode = *m,
            Action::ToggleSpotlight => {
                if c.spotlight.mode == fx_core::SpotlightMode::Off {
                    c.spotlight.mode = c.spotlight.shape;
                    c.enabled = true;
                } else {
                    c.spotlight.mode = fx_core::SpotlightMode::Off;
                }
            }
            Action::ToggleRing => c.ring.enabled = !c.ring.enabled,
            Action::TogglePulse => c.ring.pulse = !c.ring.pulse,
            Action::ToggleClicks => c.clicks.enabled = !c.clicks.enabled,
            Action::ToggleClickSound => c.clicks.sound = !c.clicks.sound,
            Action::ToggleTrail => c.trail.enabled = !c.trail.enabled,
            Action::ToggleToolbar => c.toolbar.visible = !c.toolbar.visible,
            Action::ToggleKeys => c.keys.enabled = !c.keys.enabled,
            Action::ToggleRecordAudio(0) => c.record.system_audio = !c.record.system_audio,
            Action::ToggleRecordAudio(_) => c.record.mic = !c.record.mic,
            Action::ToolbarMoved { x, y } => {
                c.toolbar.x = Some(*x);
                c.toolbar.y = Some(*y);
            }
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
        Action::ToggleAll => t(if c.enabled { "Clicksy : effets activés" } else { "Clicksy : effets en pause" }).into(),
        Action::CycleSpotlight | Action::SetSpotlight(_) | Action::ToggleSpotlight => tf("Spotlight : {}", &[&t(c.spotlight.mode.label_fr()).to_lowercase()]),
        Action::ToggleRing => tf("Anneau {}", &[on_off(c.ring.enabled)]),
        Action::TogglePulse => tf("Pulsation {}", &[t(if c.ring.pulse { "activée" } else { "désactivée" })]),
        Action::ToggleClicks => tf("Effets de clic {}", &[on_off(c.clicks.enabled)]),
        Action::ToggleClickSound => tf("Son de clic {}", &[on_off(c.clicks.sound)]),
        Action::ToggleTrail => tf("Traînée {}", &[t(if c.trail.enabled { "activée" } else { "désactivée" })]),
        Action::ToggleToolbar => tf("Barre d'outils {}", &[t(if c.toolbar.visible { "affichée" } else { "masquée" })]),
        Action::ToggleKeys => tf("Affichage des touches {}", &[on_off(c.keys.enabled)]),
        _ => return None,
    })
}

/// Applique le profil n° `i` et l'enregistre comme profil actif.
fn apply_profile(shared: &Shared, i: usize) {
    let Some(p) = shared.config.load().profiles.get(i).cloned() else { return };
    shared.update_config(|c| c.apply_profile(&p));
    let _ = paths::save_config(&shared.data_dir, &shared.config.load_full());
    shared.banner(tf("Profil : {}", &[&crate::profile_name(&p.name)]));
}
