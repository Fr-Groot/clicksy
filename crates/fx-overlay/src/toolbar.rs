//! Barre d'outils flottante pour le formateur.
//!
//! - Fenêtre native rendue au GPU (comme la surcouche), thème sombre fixe.
//! - Ne prend **jamais** le focus (WS_EX_NOACTIVATE + MA_NOACTIVATE) : cliquer dessus ne fait
//!   pas sortir PowerPoint du diaporama et ne vole pas le clavier de l'appli active.
//! - Déplaçable par sa poignée, aimantée aux bords gauche/droit de l'écran.
//! - Optionnellement exclue des captures et partages (WDA_EXCLUDEFROMCAPTURE).

use crate::draw::d2d;
use crate::gpu::{Gpu, Layer};
use fx_bus::Action;
use fx_core::{Color, Config, DrawTool, Shared, SpotlightMode};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::DXGI_PRESENT;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_numerics::{Matrix3x2, Vector2};

const CLASS: PCWSTR = w!("ClicksyToolbar");
/// Défini dans Win32::UI::Controls, qu'on évite d'importer pour une seule constante.
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Dimensions en DIP.
const W: f32 = 112.0;
const PAD: f32 = 6.0;
const GRIP_H: f32 = 22.0;
const BTN_H: f32 = 50.0;
const COLORS_H: f32 = 34.0;
const SEP_H: f32 = 9.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Btn {
    Mouse,
    Pen,
    Rect,
    Ellipse,
    Arrow,
    Width,
    Undo,
    Clear,
    White,
    Black,
    Record,
    Capture,
    Laser,
    Spot,
    Settings,
    Hide,
}

impl Btn {
    fn label(self) -> &'static str {
        match self {
            Btn::Mouse => "Souris",
            Btn::Pen => "Stylo",
            Btn::Rect => "Cadre",
            Btn::Ellipse => "Rond",
            Btn::Arrow => "Flèche",
            Btn::Width => "Trait",
            Btn::Undo => "Annuler",
            Btn::Clear => "Effacer",
            Btn::White => "Blanc",
            Btn::Black => "Noir",
            Btn::Record => "Vidéo",
            Btn::Capture => "Capture",
            Btn::Laser => "Laser",
            Btn::Spot => "Spot",
            Btn::Settings => "Réglages",
            Btn::Hide => "Masquer",
        }
    }

    fn action(self) -> Action {
        match self {
            Btn::Mouse => Action::ExitDraw,
            Btn::Pen => Action::SetTool(0),
            Btn::Rect => Action::SetTool(1),
            Btn::Ellipse => Action::SetTool(2),
            Btn::Arrow => Action::SetTool(3),
            Btn::Width => Action::CycleWidth,
            Btn::Undo => Action::Undo,
            Btn::Clear => Action::ClearDrawings,
            Btn::White => Action::SetBoard(1),
            Btn::Black => Action::SetBoard(2),
            // Géré à part (menu de choix de la source), voir `record_click`.
            Btn::Record => Action::RecordStop,
            Btn::Capture => Action::Screenshot,
            Btn::Laser => Action::ToggleLaser,
            Btn::Spot => Action::ToggleSpotlight,
            Btn::Settings => Action::OpenSettings,
            Btn::Hide => Action::ToggleToolbar,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Hit {
    Grip,
    Btn(Btn),
    Color(u8),
}

/// Zones cliquables, en DIP relatifs à la fenêtre.
fn layout() -> (Vec<(Hit, D2D_RECT_F)>, Vec<f32>, f32) {
    let mut items = Vec::new();
    let mut seps = Vec::new();
    let mut y = PAD;
    let rect = |l: f32, t: f32, r: f32, b: f32| D2D_RECT_F { left: l, top: t, right: r, bottom: b };
    // Poignée, et petit bouton « masquer » dans le coin.
    items.push((Hit::Grip, rect(PAD, y, W - PAD - GRIP_H, y + GRIP_H)));
    items.push((Hit::Btn(Btn::Hide), rect(W - PAD - GRIP_H, y, W - PAD, y + GRIP_H)));
    y += GRIP_H;
    let col = (W - 2.0 * PAD) / 2.0;
    let pair = |items: &mut Vec<(Hit, D2D_RECT_F)>, y: &mut f32, a: Btn, b: Btn| {
        items.push((Hit::Btn(a), rect(PAD, *y, PAD + col, *y + BTN_H)));
        items.push((Hit::Btn(b), rect(PAD + col, *y, W - PAD, *y + BTN_H)));
        *y += BTN_H;
    };
    pair(&mut items, &mut y, Btn::Mouse, Btn::Pen);
    pair(&mut items, &mut y, Btn::Rect, Btn::Ellipse);
    pair(&mut items, &mut y, Btn::Arrow, Btn::Width);
    let sw = (W - 2.0 * PAD) / 3.0;
    for i in 0..3u8 {
        let l = PAD + sw * i as f32;
        items.push((Hit::Color(i), rect(l, y, l + sw, y + COLORS_H)));
    }
    y += COLORS_H;
    pair(&mut items, &mut y, Btn::Undo, Btn::Clear);
    seps.push(y + SEP_H / 2.0);
    y += SEP_H;
    pair(&mut items, &mut y, Btn::White, Btn::Black);
    pair(&mut items, &mut y, Btn::Spot, Btn::Laser);
    seps.push(y + SEP_H / 2.0);
    y += SEP_H;
    pair(&mut items, &mut y, Btn::Record, Btn::Capture);
    items.push((Hit::Btn(Btn::Settings), rect(PAD, y, W - PAD, y + BTN_H)));
    y += BTN_H;
    y += PAD;
    (items, seps, y)
}

struct Toolbar {
    hwnd: HWND,
    layer: Layer,
    /// Rectangle de la fenêtre, pixels physiques.
    rect: RECT,
    scale: f32,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    /// Décalage curseur → coin de la fenêtre pendant un déplacement.
    drag: Option<(i32, i32)>,
    excluded: bool,
    drawn_sig: u64,
    needs_rebuild: bool,
    label_format: IDWriteTextFormat,
}

thread_local! {
    static TB: RefCell<Option<Toolbar>> = const { RefCell::new(None) };
    static SHARED: RefCell<Option<Arc<Shared>>> = const { RefCell::new(None) };
}

fn shared() -> Option<Arc<Shared>> {
    SHARED.with(|s| s.borrow().clone())
}

pub fn register(shared: Arc<Shared>) {
    SHARED.with(|s| *s.borrow_mut() = Some(shared));
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst.into(),
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    }
}

/// Fenêtre de la barre (pour la remettre au premier plan).
pub fn raise() {
    TB.with(|t| {
        if let Some(tb) = t.borrow().as_ref() {
            unsafe {
                let _ = SetWindowPos(tb.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
            }
        }
    });
}

/// Détruit la barre (changement d'écrans, perte du GPU, arrêt).
pub fn destroy(shared: &Shared) {
    // On sort la barre du RefCell avant de la détruire : DestroyWindow renvoie des messages
    // à wnd_proc, qui doit pouvoir emprunter TB.
    let old = TB.with(|t| t.borrow_mut().take());
    drop(old);
    shared.modes.set_toolbar_rect(None);
}

/// État visible sur la barre ; quand il change, on la redessine.
pub struct View {
    pub draw_mode: bool,
    pub tool: DrawTool,
    pub color_index: usize,
    pub width: f32,
    pub laser: bool,
    pub board: u8,
    pub recording: bool,
    /// Durée de l'enregistrement en cours, en secondes.
    pub rec_secs: Option<u64>,
    pub countdown: bool,
}

/// Crée, met à jour ou supprime la barre selon la config, puis la redessine si besoin.
/// Renvoie `true` si une image a été présentée.
pub fn sync(gpu: &Gpu, shared: &Shared, cfg: &Config, view: &View) -> Result<bool> {
    if !cfg.toolbar.visible {
        if TB.with(|t| t.borrow().is_some()) {
            destroy(shared);
        }
        return Ok(false);
    }
    let rebuild = TB.with(|t| t.borrow().as_ref().is_some_and(|tb| tb.needs_rebuild));
    if rebuild {
        destroy(shared);
    }
    if TB.with(|t| t.borrow().is_none()) {
        let tb = unsafe { create(gpu, cfg)? };
        shared.modes.set_toolbar_rect(Some((tb.rect.left, tb.rect.top, tb.rect.right, tb.rect.bottom)));
        TB.with(|t| *t.borrow_mut() = Some(tb));
    }

    TB.with(|t| {
        let mut b = t.borrow_mut();
        let tb = b.as_mut().unwrap();
        // Toujours exclue pendant un enregistrement vidéo.
        let exclude = cfg.toolbar.hide_from_capture || view.recording;
        if tb.excluded != exclude {
            unsafe { set_excluded(tb.hwnd, exclude) };
            tb.excluded = exclude;
        }
        let sig = signature(tb, cfg, view);
        if sig == tb.drawn_sig {
            return Ok(false);
        }
        unsafe { paint(gpu, tb, cfg, view)? };
        tb.drawn_sig = sig;
        Ok(true)
    })
}

fn signature(tb: &Toolbar, cfg: &Config, v: &View) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (tb.hover, tb.pressed, tb.scale.to_bits(), v.draw_mode, v.tool as u8, v.color_index, v.width.to_bits()).hash(&mut h);
    (v.laser, v.board, cfg.spotlight.mode as u8, cfg.spotlight.shape as u8).hash(&mut h);
    (v.recording, v.rec_secs, v.countdown).hash(&mut h);
    for c in &cfg.draw.colors {
        c.to_hex().hash(&mut h);
    }
    h.finish()
}

unsafe fn set_excluded(hwnd: HWND, on: bool) {
    let affinity = if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
    if SetWindowDisplayAffinity(hwnd, affinity).is_err() && on {
        // Windows trop ancien (< 10 2004) : la barre apparaît en noir dans les captures.
        let _ = SetWindowDisplayAffinity(hwnd, WDA_MONITOR);
    }
}

fn monitor_scale(hmon: HMONITOR) -> f32 {
    let (mut dx, mut dy) = (96u32, 96u32);
    unsafe {
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    }
    dx as f32 / 96.0
}

fn work_area(hmon: HMONITOR) -> RECT {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(hmon, &mut info);
    }
    info.rcWork
}

/// Garde la barre dans la zone de travail d'un écran et l'aimante aux bords proches.
fn place(x: i32, y: i32, w: i32, h: i32, snap: bool) -> (i32, i32, HMONITOR) {
    let center = POINT { x: x + w / 2, y: y + h / 2 };
    let hmon = unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST) };
    let wa = work_area(hmon);
    let margin = (8.0 * monitor_scale(hmon)) as i32;
    let mut nx = x.clamp(wa.left, (wa.right - w).max(wa.left));
    let ny = y.clamp(wa.top, (wa.bottom - h).max(wa.top));
    if snap {
        let zone = (60.0 * monitor_scale(hmon)) as i32;
        if nx - wa.left < zone {
            nx = wa.left + margin;
        } else if wa.right - (nx + w) < zone {
            nx = wa.right - w - margin;
        }
    }
    (nx, ny, hmon)
}

unsafe fn create(gpu: &Gpu, cfg: &Config) -> Result<Toolbar> {
    let (_, _, total_h) = layout();
    // Position mémorisée, sinon bord droit de l'écran principal, centrée verticalement.
    let (x0, y0) = match (cfg.toolbar.x, cfg.toolbar.y) {
        (Some(x), Some(y)) => (x, y),
        _ => {
            let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let wa = work_area(hmon);
            let s = monitor_scale(hmon);
            let (w, h) = ((W * s) as i32, (total_h * s) as i32);
            (wa.right - w - (8.0 * s) as i32, wa.top + (wa.bottom - wa.top - h) / 2)
        }
    };
    let hmon = MonitorFromPoint(POINT { x: x0 + 10, y: y0 + 10 }, MONITOR_DEFAULTTONEAREST);
    let scale = monitor_scale(hmon);
    let (w, h) = ((W * scale).round() as i32, (total_h * scale).round() as i32);
    let (x, y, _) = place(x0, y0, w, h, false);

    let hinst = GetModuleHandleW(None)?;
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
        CLASS,
        w!("Clicksy – barre d'outils"),
        WS_POPUP,
        x,
        y,
        w,
        h,
        None,
        None,
        Some(hinst.into()),
        None,
    )?;
    let layer = Layer::new(gpu, hwnd, w as u32, h as u32)?;
    if cfg.toolbar.hide_from_capture {
        set_excluded(hwnd, true);
    }
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    let label_format = gpu.dwrite.CreateTextFormat(
        w!("Segoe UI"),
        None,
        DWRITE_FONT_WEIGHT_NORMAL,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        10.5,
        w!("fr-FR"),
    )?;
    label_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
    label_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
    Ok(Toolbar {
        hwnd,
        layer,
        rect: RECT { left: x, top: y, right: x + w, bottom: y + h },
        scale,
        hover: None,
        pressed: None,
        drag: None,
        excluded: cfg.toolbar.hide_from_capture,
        drawn_sig: 0,
        needs_rebuild: false,
        label_format,
    })
}

impl Drop for Toolbar {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

// ---------------------------------------------------------------- Dessin

fn v(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

const WHITE: D2D1_COLOR_F = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.92 };
const ACCENT: Color = Color::rgba8(0x2F, 0x7C, 0xF6, 255);
const REC_RED: Color = Color::rgba8(0xE5, 0x2B, 0x35, 255);

fn is_active(b: Btn, cfg: &Config, v: &View) -> bool {
    match b {
        Btn::Mouse => !v.draw_mode,
        Btn::Pen => v.draw_mode && v.tool == DrawTool::Pen,
        Btn::Rect => v.draw_mode && v.tool == DrawTool::Rect,
        Btn::Ellipse => v.draw_mode && v.tool == DrawTool::Ellipse,
        Btn::Arrow => v.draw_mode && v.tool == DrawTool::Arrow,
        Btn::White => v.board == 1,
        Btn::Black => v.board == 2,
        Btn::Record => v.recording || v.countdown,
        Btn::Laser => v.laser,
        Btn::Spot => cfg.spotlight.mode != SpotlightMode::Off,
        _ => false,
    }
}

unsafe fn paint(gpu: &Gpu, tb: &Toolbar, cfg: &Config, view: &View) -> Result<()> {
    let dc = &gpu.dc;
    let s = tb.scale;
    let (items, seps, total_h) = layout();
    dc.SetTarget(&tb.layer.target);
    dc.BeginDraw();
    dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
    dc.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: 0.0, M32: 0.0 });

    // Fond arrondi + liseré.
    let bg = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: 0.5, top: 0.5, right: W - 0.5, bottom: total_h - 0.5 },
        radiusX: 12.0,
        radiusY: 12.0,
    };
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 0.11, g: 0.11, b: 0.13, a: 1.0 });
    dc.FillRoundedRectangle(&bg, &gpu.brush);
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.14 });
    dc.DrawRoundedRectangle(&bg, &gpu.brush, 1.0, None);

    for y in seps {
        gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.10 });
        dc.DrawLine(v(PAD + 6.0, y), v(W - PAD - 6.0, y), &gpu.brush, 1.0, None);
    }

    for (hit, r) in &items {
        let hovered = tb.hover == Some(*hit);
        let pressed = tb.pressed == Some(*hit) && hovered;
        let cell = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: r.left + 2.0, top: r.top + 2.0, right: r.right - 2.0, bottom: r.bottom - 2.0 },
            radiusX: 7.0,
            radiusY: 7.0,
        };
        match hit {
            Hit::Grip => {
                // Six points de préhension.
                gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if hovered { 0.75 } else { 0.4 } });
                let cx = (r.left + r.right) / 2.0;
                let cy = (r.top + r.bottom) / 2.0;
                for i in -1..=1 {
                    for j in [-1.0f32, 1.0] {
                        let e = D2D1_ELLIPSE { point: v(cx + i as f32 * 6.0, cy + j * 2.5), radiusX: 1.3, radiusY: 1.3 };
                        dc.FillEllipse(&e, &gpu.brush);
                    }
                }
            }
            Hit::Color(i) => {
                let c = cfg.draw.colors[*i as usize];
                let cx = (r.left + r.right) / 2.0;
                let cy = (r.top + r.bottom) / 2.0;
                if view.color_index == *i as usize {
                    gpu.brush.SetColor(&WHITE);
                    let e = D2D1_ELLIPSE { point: v(cx, cy), radiusX: 11.0, radiusY: 11.0 };
                    dc.DrawEllipse(&e, &gpu.brush, 2.0, None);
                } else if hovered {
                    gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.35 });
                    let e = D2D1_ELLIPSE { point: v(cx, cy), radiusX: 11.0, radiusY: 11.0 };
                    dc.DrawEllipse(&e, &gpu.brush, 1.5, None);
                }
                gpu.brush.SetColor(&d2d(c));
                let e = D2D1_ELLIPSE { point: v(cx, cy), radiusX: 7.5, radiusY: 7.5 };
                dc.FillEllipse(&e, &gpu.brush);
            }
            Hit::Btn(b) => {
                let active = is_active(*b, cfg, view);
                if active {
                    let accent = if *b == Btn::Record { REC_RED } else { ACCENT };
                    gpu.brush.SetColor(&d2d(accent.with_alpha(if pressed { 0.75 } else { 0.95 })));
                    dc.FillRoundedRectangle(&cell, &gpu.brush);
                } else if hovered {
                    gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if pressed { 0.18 } else { 0.09 } });
                    dc.FillRoundedRectangle(&cell, &gpu.brush);
                }
                let cx = (r.left + r.right) / 2.0;
                // Petit bouton sans libellé (masquer, dans la poignée).
                if r.bottom - r.top < BTN_H {
                    draw_icon(gpu, *b, cx, (r.top + r.bottom) / 2.0, cfg, view);
                    continue;
                }
                let icon_y = r.top + 19.0;
                draw_icon(gpu, *b, cx, icon_y, cfg, view);
                // Le bouton vidéo affiche le chrono pendant l'enregistrement.
                let text = match (*b, view.rec_secs, view.countdown) {
                    (Btn::Record, Some(s), _) => format!("{}:{:02}", s / 60, s % 60),
                    (Btn::Record, None, true) => "Annuler".to_string(),
                    _ => b.label().to_string(),
                };
                let label: Vec<u16> = text.encode_utf16().collect();
                let lr = D2D_RECT_F { left: r.left, top: r.top + 31.0, right: r.right, bottom: r.bottom - 3.0 };
                gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if active { 1.0 } else { 0.78 } });
                dc.DrawText(&label, &tb.label_format, &lr, &gpu.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
        }
    }

    dc.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 });
    let res = dc.EndDraw(None, None);
    dc.SetTarget(None);
    res?;
    tb.layer.swapchain.Present(0, DXGI_PRESENT(0)).ok()
}

/// Icônes vectorielles (DIP), centrées sur (cx, cy), environ 18 × 18.
unsafe fn draw_icon(gpu: &Gpu, b: Btn, cx: f32, cy: f32, cfg: &Config, view: &View) {
    let dc = &gpu.dc;
    let br = &gpu.brush;
    let st = &gpu.round;
    let lw = 1.7;
    br.SetColor(&WHITE);
    let line = |a: (f32, f32), z: (f32, f32), w: f32| dc.DrawLine(v(cx + a.0, cy + a.1), v(cx + z.0, cy + z.1), br, w, st);
    match b {
        Btn::Mouse => {
            let pts = [(-4.0, -8.0), (-4.0, 6.0), (-0.6, 3.0), (1.8, 8.2), (4.0, 7.2), (1.6, 2.2), (6.0, 2.2)];
            for i in 0..pts.len() {
                line(pts[i], pts[(i + 1) % pts.len()], lw);
            }
        }
        Btn::Pen => {
            line((-6.5, 6.5), (5.0, -5.0), 4.2);
            br.SetColor(&D2D1_COLOR_F { r: 0.11, g: 0.11, b: 0.13, a: 1.0 });
            line((-6.0, 6.0), (4.6, -4.6), 1.4);
            br.SetColor(&WHITE);
            line((-8.0, 8.0), (-6.5, 6.5), 2.0);
        }
        Btn::Rect => {
            let r = D2D_RECT_F { left: cx - 8.0, top: cy - 6.0, right: cx + 8.0, bottom: cy + 6.0 };
            dc.DrawRectangle(&r, br, lw, st);
        }
        Btn::Ellipse => {
            let e = D2D1_ELLIPSE { point: v(cx, cy), radiusX: 8.5, radiusY: 6.5 };
            dc.DrawEllipse(&e, br, lw, st);
        }
        Btn::Arrow => {
            line((-7.0, 7.0), (7.0, -7.0), lw);
            line((7.0, -7.0), (0.5, -7.0), lw);
            line((7.0, -7.0), (7.0, -0.5), lw);
        }
        Btn::Width => {
            let r = (view.width * 0.55).clamp(1.8, 8.0);
            br.SetColor(&d2d(cfg.draw.colors[view.color_index.min(2)]));
            let e = D2D1_ELLIPSE { point: v(cx, cy), radiusX: r, radiusY: r };
            dc.FillEllipse(&e, br);
        }
        Btn::Undo => {
            // Arc de cercle et pointe.
            let n = 10;
            let mut prev = None;
            for i in 0..=n {
                let a = std::f32::consts::PI * (1.0 + i as f32 / n as f32 * 1.15);
                let p = (6.0 * a.cos(), 2.0 + 6.0 * a.sin());
                if let Some(q) = prev {
                    line(q, p, lw);
                }
                prev = Some(p);
            }
            line((-6.0, 2.0), (-8.6, -1.6), lw);
            line((-6.0, 2.0), (-2.4, 0.4), lw);
        }
        Btn::Clear => {
            line((-7.0, -5.0), (7.0, -5.0), lw);
            line((-2.5, -5.0), (-1.5, -7.5), lw);
            line((-1.5, -7.5), (1.5, -7.5), lw);
            line((1.5, -7.5), (2.5, -5.0), lw);
            let pts = [(-5.5, -5.0), (-4.5, 8.0), (4.5, 8.0), (5.5, -5.0)];
            for w in pts.windows(2) {
                line(w[0], w[1], lw);
            }
            line((-1.8, -1.5), (-1.5, 5.0), 1.3);
            line((1.8, -1.5), (1.5, 5.0), 1.3);
        }
        Btn::White | Btn::Black => {
            // Chevalet : tableau plein blanc, ou noir cerclé de blanc.
            let r = D2D_RECT_F { left: cx - 8.5, top: cy - 7.5, right: cx + 8.5, bottom: cy + 4.0 };
            if b == Btn::Black {
                br.SetColor(&D2D1_COLOR_F { r: 0.02, g: 0.02, b: 0.02, a: 1.0 });
            }
            dc.FillRectangle(&r, br);
            br.SetColor(&WHITE);
            dc.DrawRectangle(&r, br, lw, st);
            line((-4.0, 4.0), (-6.5, 8.5), lw);
            line((4.0, 4.0), (6.5, 8.5), lw);
        }
        Btn::Record => {
            if view.recording || view.countdown {
                // Carré « stop ».
                let r = D2D_RECT_F { left: cx - 5.5, top: cy - 5.5, right: cx + 5.5, bottom: cy + 5.5 };
                dc.FillRectangle(&r, br);
            } else {
                br.SetColor(&d2d(REC_RED));
                dc.FillEllipse(&D2D1_ELLIPSE { point: v(cx, cy), radiusX: 6.5, radiusY: 6.5 }, br);
                br.SetColor(&WHITE);
                dc.DrawEllipse(&D2D1_ELLIPSE { point: v(cx, cy), radiusX: 9.0, radiusY: 9.0 }, br, 1.4, st);
            }
        }
        Btn::Capture => {
            let body = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: cx - 8.5, top: cy - 4.5, right: cx + 8.5, bottom: cy + 7.0 },
                radiusX: 2.0,
                radiusY: 2.0,
            };
            dc.DrawRoundedRectangle(&body, br, lw, st);
            line((-3.5, -4.5), (-2.0, -7.0), lw);
            line((-2.0, -7.0), (2.0, -7.0), lw);
            line((2.0, -7.0), (3.5, -4.5), lw);
            let e = D2D1_ELLIPSE { point: v(cx, cy + 1.2), radiusX: 3.3, radiusY: 3.3 };
            dc.DrawEllipse(&e, br, lw, st);
        }
        Btn::Laser => {
            let c = cfg.laser.color;
            br.SetColor(&d2d(c.with_alpha(0.35)));
            let e = D2D1_ELLIPSE { point: v(cx, cy), radiusX: 7.5, radiusY: 7.5 };
            dc.FillEllipse(&e, br);
            br.SetColor(&d2d(c));
            let e = D2D1_ELLIPSE { point: v(cx, cy), radiusX: 4.0, radiusY: 4.0 };
            dc.FillEllipse(&e, br);
            br.SetColor(&WHITE);
            let e = D2D1_ELLIPSE { point: v(cx, cy), radiusX: 1.6, radiusY: 1.6 };
            dc.FillEllipse(&e, br);
        }
        Btn::Spot => {
            // L'icône montre la forme choisie dans les réglages : zone claire dans un cadre sombre.
            let frame = D2D_RECT_F { left: cx - 9.0, top: cy - 7.0, right: cx + 9.0, bottom: cy + 7.0 };
            br.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.25 });
            dc.FillRectangle(&frame, br);
            br.SetColor(&WHITE);
            match cfg.spotlight.shape {
                SpotlightMode::HBand => dc.FillRectangle(&D2D_RECT_F { left: cx - 9.0, top: cy - 2.5, right: cx + 9.0, bottom: cy + 2.5 }, br),
                SpotlightMode::VBand => dc.FillRectangle(&D2D_RECT_F { left: cx - 2.5, top: cy - 7.0, right: cx + 2.5, bottom: cy + 7.0 }, br),
                _ => dc.FillEllipse(&D2D1_ELLIPSE { point: v(cx, cy), radiusX: 4.5, radiusY: 4.5 }, br),
            }
            dc.DrawRectangle(&frame, br, 1.2, st);
        }
        Btn::Settings => {
            for (i, k) in [(-5.0f32, -3.0f32), (0.0, 3.5), (5.0, -1.0)] {
                line((-8.0, i), (8.0, i), lw);
                let e = D2D1_ELLIPSE { point: v(cx + k, cy + i), radiusX: 2.3, radiusY: 2.3 };
                br.SetColor(&D2D1_COLOR_F { r: 0.11, g: 0.11, b: 0.13, a: 1.0 });
                dc.FillEllipse(&e, br);
                br.SetColor(&WHITE);
                dc.DrawEllipse(&e, br, 1.5, None);
            }
        }
        Btn::Hide => {
            // Petit « réduire » dans le coin de la poignée.
            br.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.6 });
            line((-4.5, 0.0), (4.5, 0.0), 1.6);
        }
    }
}

// ---------------------------------------------------------------- Souris

fn hit_test(tb: &Toolbar, px: i32, py: i32) -> Option<Hit> {
    let (x, y) = (px as f32 / tb.scale, py as f32 / tb.scale);
    layout().0.into_iter().find(|(_, r)| x >= r.left && x < r.right && y >= r.top && y < r.bottom).map(|(h, _)| h)
}

fn lparam_point(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32)
}

fn request_paint() {
    if let Some(s) = shared() {
        s.wake();
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Jamais de focus : PowerPoint reste en diaporama, le clavier reste à l'appli active.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            let (x, y) = lparam_point(lparam);
            let mut moved_to = None;
            TB.with(|t| {
                if let Some(tb) = t.borrow_mut().as_mut() {
                    if let Some((ox, oy)) = tb.drag {
                        let mut pt = POINT::default();
                        let _ = GetCursorPos(&mut pt);
                        moved_to = Some((pt.x - ox, pt.y - oy));
                    } else {
                        let h = hit_test(tb, x, y);
                        if h != tb.hover {
                            tb.hover = h;
                            request_paint();
                        }
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut tme);
                    }
                }
            });
            if let Some((nx, ny)) = moved_to {
                move_to(hwnd, nx, ny, false);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            TB.with(|t| {
                if let Some(tb) = t.borrow_mut().as_mut() {
                    if tb.drag.is_none() {
                        tb.hover = None;
                        tb.pressed = None;
                        request_paint();
                    }
                }
            });
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let grip = TB.with(|t| t.borrow().as_ref().is_some_and(|tb| tb.hover == Some(Hit::Grip) || tb.drag.is_some()));
            let id = if grip { IDC_SIZEALL } else { IDC_HAND };
            let _ = SetCursor(LoadCursorW(None, id).ok());
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_point(lparam);
            let mut start_drag = false;
            TB.with(|t| {
                if let Some(tb) = t.borrow_mut().as_mut() {
                    let h = hit_test(tb, x, y);
                    if h == Some(Hit::Grip) {
                        tb.drag = Some((x, y));
                        start_drag = true;
                    } else {
                        tb.pressed = h;
                    }
                }
            });
            if start_drag {
                if let Some(s) = shared() {
                    s.modes.toolbar_drag.store(true, Ordering::Release);
                }
                SetCapture(hwnd);
            }
            request_paint();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = lparam_point(lparam);
            let mut action = None;
            let mut drag_end = None;
            TB.with(|t| {
                if let Some(tb) = t.borrow_mut().as_mut() {
                    if tb.drag.take().is_some() {
                        drag_end = Some((tb.rect.left, tb.rect.top));
                    } else {
                        let h = hit_test(tb, x, y);
                        if h.is_some() && h == tb.pressed {
                            action = match h {
                                Some(Hit::Btn(b)) => Some(b.action()),
                                Some(Hit::Color(i)) => Some(Action::SetColor(i)),
                                _ => None,
                            };
                        }
                        tb.pressed = None;
                    }
                }
            });
            if let Some((l, t)) = drag_end {
                let _ = ReleaseCapture();
                if let Some(s) = shared() {
                    s.modes.toolbar_drag.store(false, Ordering::Release);
                }
                if let Some((nx, ny)) = move_to(hwnd, l, t, true) {
                    fx_bus::action(Action::ToolbarMoved { x: nx, y: ny });
                }
            }
            if let Some(a) = action {
                if a == Action::RecordStop {
                    record_click(hwnd);
                } else {
                    fx_bus::action(a);
                }
            }
            request_paint();
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            TB.with(|t| {
                if let Some(tb) = t.borrow_mut().as_mut() {
                    tb.drag = None;
                }
            });
            if let Some(s) = shared() {
                s.modes.toolbar_drag.store(false, Ordering::Release);
            }
            LRESULT(0)
        }
        // On gère nous-mêmes le changement d'échelle (recréation à la bonne taille).
        WM_DPICHANGED => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Déplace la barre ; `snap` à la fin d'un glisser. Renvoie la position finale.
unsafe fn move_to(hwnd: HWND, x: i32, y: i32, snap: bool) -> Option<(i32, i32)> {
    let (w, h, old_scale) = TB.with(|t| {
        t.borrow().as_ref().map(|tb| (tb.rect.right - tb.rect.left, tb.rect.bottom - tb.rect.top, tb.scale))
    })?;
    let (nx, ny, hmon) = if snap { place(x, y, w, h, true) } else { (x, y, HMONITOR::default()) };
    let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), nx, ny, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
    TB.with(|t| {
        if let Some(tb) = t.borrow_mut().as_mut() {
            tb.rect = RECT { left: nx, top: ny, right: nx + w, bottom: ny + h };
            // Déposée sur un écran d'une autre échelle : on la recrée à la bonne taille.
            if snap && (monitor_scale(hmon) - old_scale).abs() > 0.01 {
                tb.needs_rebuild = true;
            }
        }
    });
    if let Some(s) = shared() {
        s.modes.set_toolbar_rect(Some((nx, ny, nx + w, ny + h)));
        s.wake();
    }
    Some((nx, ny))
}

/// Bouton « Vidéo » : arrête l'enregistrement en cours, sinon propose la source.
unsafe fn record_click(hwnd: HWND) {
    let Some(shared) = shared() else { return };
    let busy = shared.modes.recording.load(Ordering::Acquire) || shared.fx.lock().unwrap().countdown.is_some();
    if busy {
        fx_bus::action(Action::RecordStop);
        return;
    }
    let cfg = shared.config.load_full();
    let Ok(menu) = CreatePopupMenu() else { return };
    let add = |id: usize, text: &str, checked: bool| {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let flags = if checked { MF_STRING | MF_CHECKED } else { MF_STRING };
        let _ = AppendMenuW(menu, flags, id, PCWSTR(wide.as_ptr()));
    };
    add(1, "Enregistrer l'écran entier", false);
    add(2, "Enregistrer une fenêtre…", false);
    add(3, "Enregistrer une zone…", false);
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    add(4, "Son du PC", cfg.record.system_audio);
    add(5, "Micro", cfg.record.mic);
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    add(6, "Ouvrir le dossier des vidéos", false);

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    // Le menu a besoin du premier plan le temps du choix (seul moment où la barre prend le focus).
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None);
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
    match cmd.0 {
        1 => fx_bus::action(Action::RecordStart(fx_bus::RecordTarget::Screen)),
        2 => fx_bus::action(Action::RecordPick(1)),
        3 => fx_bus::action(Action::RecordPick(2)),
        4 => fx_bus::action(Action::ToggleRecordAudio(0)),
        5 => fx_bus::action(Action::ToggleRecordAudio(1)),
        6 => fx_bus::action(Action::OpenVideosFolder),
        _ => {}
    }
}
