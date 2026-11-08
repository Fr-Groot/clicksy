//! Dessin d'une image sur une surface : spotlight, anneau, effets de clic.
//! Tout est calculé en pixels physiques ; les tailles de la config (DIP) sont
//! multipliées par l'échelle de l'écran où se trouve le curseur.

use crate::annot;
use crate::gpu::{Gpu, Surface};
use fx_bus::MouseButton;
use fx_core::{Color, Config, DrawTool, Ripple, Shape, SpotlightMode};
use std::time::Instant;
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;

pub struct Frame<'a> {
    pub cfg: &'a Config,
    pub cursor: (i32, i32),
    /// Échelle de l'écran qui contient le curseur.
    pub scale: f32,
    pub now: Instant,
    pub ripples: &'a [Ripple],
    pub held: [bool; 3],
    /// Phase de la pulsation de l'anneau, 0..1.
    pub pulse: f32,
    /// Formes terminées + forme en cours.
    pub shapes: &'a [Shape],
    pub draw_mode: bool,
    pub draw_tool: DrawTool,
    pub draw_color: Color,
    pub laser: bool,
    /// Points de la traînée (x, y, fraction de vie restante 0..1), du plus ancien au plus récent.
    pub trail: &'a [(f32, f32, f32)],
    /// Texte de la bulle et son opacité.
    pub banner: Option<(&'a str, f32)>,
    /// Tableau plein écran : 0 aucun, 1 blanc, 2 noir, sur l'écran qui contient `board_pt`.
    pub board: u8,
    pub board_pt: (i32, i32),
}

impl Frame<'_> {
    fn board_on(&self, s: &Surface) -> bool {
        self.board != 0 && s.contains(self.board_pt.0, self.board_pt.1)
    }
}

impl Frame<'_> {
    /// L'anneau est masqué en mode laser et en mode dessin (on veut un pointeur précis).
    fn ring_visible(&self) -> bool {
        self.cfg.ring.enabled && !self.laser && !self.draw_mode
    }
}

impl Frame<'_> {
    fn ring_radius(&self) -> f32 {
        let r = &self.cfg.ring;
        let k = if r.pulse { 1.0 + 0.18 * (self.pulse * std::f32::consts::TAU).sin() } else { 1.0 };
        r.radius * self.scale * k
    }

    fn button_color(&self, b: MouseButton) -> Color {
        let c = &self.cfg.clicks;
        match b {
            MouseButton::Left => c.left_color,
            MouseButton::Right => c.right_color,
            _ => c.middle_color,
        }
    }
}

/// Le contenu de cette surface dépend-il de la position du curseur ou d'une animation ?
pub fn is_dynamic(s: &Surface, f: &Frame) -> bool {
    let cfg = f.cfg;
    let (cx, cy) = (f.cursor.0 as f32, f.cursor.1 as f32);
    let (l, t, r, b) = (s.rect.left as f32, s.rect.top as f32, s.rect.right as f32, s.rect.bottom as f32);
    let near = |x: f32, y: f32, m: f32| x + m >= l && x - m < r && y + m >= t && y - m < b;

    // Modes explicites : actifs même quand les effets sont en pause.
    if f.draw_mode || (f.banner.is_some() && s.contains(f.cursor.0, f.cursor.1)) {
        return true;
    }
    let margin = 60.0 * f.scale;
    if f.laser && near(cx, cy, cfg.laser.size * f.scale * 4.0) {
        return true;
    }
    if f.trail.iter().any(|p| near(p.0, p.1, margin)) {
        return true;
    }
    if !cfg.enabled {
        return false;
    }

    let sp = &cfg.spotlight;
    let feather = sp.feather * f.scale;
    match sp.mode {
        SpotlightMode::Off => {}
        SpotlightMode::Circle => {
            if near(cx, cy, sp.radius * f.scale + feather) {
                return true;
            }
        }
        SpotlightMode::HBand => {
            let m = sp.band_size * f.scale / 2.0 + feather;
            if cy + m >= t && cy - m < b {
                return true;
            }
        }
        SpotlightMode::VBand => {
            let m = sp.band_size * f.scale / 2.0 + feather;
            if cx + m >= l && cx - m < r {
                return true;
            }
        }
    }
    if f.ring_visible() && near(cx, cy, f.ring_radius() * 1.2 + cfg.ring.thickness * f.scale * 2.0) {
        return true;
    }
    if cfg.clicks.enabled {
        let m = cfg.clicks.radius * f.scale * 1.2 + cfg.clicks.thickness * f.scale * 2.0;
        if f.held.iter().any(|h| *h) && near(cx, cy, m) {
            return true;
        }
        if f.ripples.iter().any(|rp| near(rp.x as f32, rp.y as f32, m)) {
            return true;
        }
    }
    false
}

pub(crate) fn d2d(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.r, g: c.g, b: c.b, a: c.a }
}

pub(crate) fn ellipse(x: f32, y: f32, r: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE { point: windows_numerics::Vector2 { X: x, Y: y }, radiusX: r, radiusY: r }
}

pub fn draw(gpu: &Gpu, s: &Surface, f: &Frame) -> Result<()> {
    let dc = &gpu.dc;
    unsafe {
        dc.SetTarget(&s.target);
        dc.BeginDraw();
        dc.SetTransform(&windows_numerics::Matrix3x2::identity());
        dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));

        // Coordonnées du curseur relatives à cet écran.
        let cx = (f.cursor.0 - s.rect.left) as f32;
        let cy = (f.cursor.1 - s.rect.top) as f32;
        if f.board_on(s) {
            // Tableau : fond opaque sur tout l'écran, les dessins par-dessus.
            let c = if f.board == 1 {
                D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
            } else {
                D2D1_COLOR_F { r: 0.11, g: 0.12, b: 0.12, a: 1.0 }
            };
            dc.Clear(Some(&c));
        } else if f.cfg.enabled {
            draw_spotlight(gpu, s, f, cx, cy)?;
        }
        annot::draw_shapes(gpu, s, f)?;
        if f.cfg.enabled || f.laser {
            annot::draw_trail(gpu, s, f);
        }
        if f.cfg.enabled {
            draw_clicks(gpu, s, f, cx, cy);
            if f.ring_visible() {
                draw_ring(gpu, f, cx, cy);
            }
        }
        if f.laser {
            annot::draw_laser(gpu, f, cx, cy)?;
        }
        if f.draw_mode {
            annot::draw_pointer(gpu, f, cx, cy);
        }
        if s.contains(f.cursor.0, f.cursor.1) {
            if let Some((text, alpha)) = f.banner {
                annot::draw_banner(gpu, s, text, alpha)?;
            }
        }

        let res = dc.EndDraw(None, None);
        dc.SetTarget(None);
        res?;
        s.swapchain.Present(0, windows::Win32::Graphics::Dxgi::DXGI_PRESENT(0)).ok()
    }
}

unsafe fn draw_spotlight(gpu: &Gpu, s: &Surface, f: &Frame, cx: f32, cy: f32) -> Result<()> {
    let sp = &f.cfg.spotlight;
    if sp.mode == SpotlightMode::Off || sp.dim <= 0.0 {
        return Ok(());
    }
    let dc = &gpu.dc;
    let dim = sp.dim_color.with_alpha(sp.dim);
    let clear = sp.dim_color.with_alpha(0.0);
    dc.Clear(Some(&d2d(dim)));

    let feather = (sp.feather * f.scale).max(1.0);
    // Mode COPY : on remplace les pixels (on « perce » le voile) au lieu de les mélanger.
    dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
    match sp.mode {
        SpotlightMode::Circle => {
            let r = sp.radius * f.scale;
            let outer = r + feather;
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: d2d(clear) },
                D2D1_GRADIENT_STOP { position: r / outer, color: d2d(clear) },
                D2D1_GRADIENT_STOP { position: 1.0, color: d2d(dim) },
            ];
            let rt: &ID2D1RenderTarget = dc;
            let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: windows_numerics::Vector2 { X: cx, Y: cy },
                gradientOriginOffset: windows_numerics::Vector2 { X: 0.0, Y: 0.0 },
                radiusX: outer,
                radiusY: outer,
            };
            let brush = dc.CreateRadialGradientBrush(&props, None, &coll)?;
            let rect = D2D_RECT_F { left: cx - outer, top: cy - outer, right: cx + outer, bottom: cy + outer };
            dc.FillRectangle(&rect, &brush);
        }
        SpotlightMode::HBand | SpotlightMode::VBand => {
            let half = sp.band_size * f.scale / 2.0;
            let total = 2.0 * (half + feather);
            let k = feather / total;
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: d2d(dim) },
                D2D1_GRADIENT_STOP { position: k, color: d2d(clear) },
                D2D1_GRADIENT_STOP { position: 1.0 - k, color: d2d(clear) },
                D2D1_GRADIENT_STOP { position: 1.0, color: d2d(dim) },
            ];
            let rt: &ID2D1RenderTarget = dc;
            let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let horizontal = sp.mode == SpotlightMode::HBand;
            let (start, end, rect) = if horizontal {
                let (a, b) = (cy - half - feather, cy + half + feather);
                ((0.0, a), (0.0, b), D2D_RECT_F { left: 0.0, top: a, right: s.width(), bottom: b })
            } else {
                let (a, b) = (cx - half - feather, cx + half + feather);
                ((a, 0.0), (b, 0.0), D2D_RECT_F { left: a, top: 0.0, right: b, bottom: s.height() })
            };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: windows_numerics::Vector2 { X: start.0, Y: start.1 },
                endPoint: windows_numerics::Vector2 { X: end.0, Y: end.1 },
            };
            let brush = dc.CreateLinearGradientBrush(&props, None, &coll)?;
            dc.FillRectangle(&rect, &brush);
        }
        SpotlightMode::Off => {}
    }
    dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
    Ok(())
}

unsafe fn draw_ring(gpu: &Gpu, f: &Frame, cx: f32, cy: f32) {
    let ring = &f.cfg.ring;
    if !ring.enabled || ring.opacity <= 0.0 {
        return;
    }
    let dc = &gpu.dc;
    let r = f.ring_radius();
    let w = ring.thickness * f.scale;
    // Liseré sombre discret : l'anneau reste lisible sur fond clair comme sur fond foncé.
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.35 * ring.opacity });
    dc.DrawEllipse(&ellipse(cx, cy, r), &gpu.brush, w + 2.0 * f.scale, None);
    gpu.brush.SetColor(&d2d(ring.color.with_alpha(ring.opacity)));
    dc.DrawEllipse(&ellipse(cx, cy, r), &gpu.brush, w, None);
}

unsafe fn draw_clicks(gpu: &Gpu, s: &Surface, f: &Frame, cx: f32, cy: f32) {
    let c = &f.cfg.clicks;
    if !c.enabled {
        return;
    }
    let dc = &gpu.dc;
    let big = c.radius * f.scale;

    if c.show_held {
        let buttons = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];
        for (i, b) in buttons.iter().enumerate() {
            if f.held[i] {
                gpu.brush.SetColor(&d2d(f.button_color(*b).with_alpha(0.45)));
                dc.FillEllipse(&ellipse(cx, cy, big * 0.38), &gpu.brush);
            }
        }
    }

    let dur = c.duration_ms as f32 / 1000.0;
    for rp in f.ripples {
        let t = (f.now.saturating_duration_since(rp.start).as_secs_f32() / dur).clamp(0.0, 1.0);
        if t >= 1.0 {
            continue;
        }
        let ease = 1.0 - (1.0 - t).powi(3);
        let radius = big * (0.25 + 0.75 * ease);
        let alpha = 1.0 - t;
        let (x, y) = ((rp.x - s.rect.left) as f32, (rp.y - s.rect.top) as f32);
        let color = f.button_color(rp.button);
        gpu.brush.SetColor(&d2d(color.with_alpha(0.18 * alpha)));
        dc.FillEllipse(&ellipse(x, y, radius), &gpu.brush);
        gpu.brush.SetColor(&d2d(color.with_alpha(alpha)));
        dc.DrawEllipse(&ellipse(x, y, radius), &gpu.brush, c.thickness * f.scale * (1.0 - 0.5 * t), None);
    }
}
