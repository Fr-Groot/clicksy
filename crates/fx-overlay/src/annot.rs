//! Dessins à l'écran (stylo, cadre, rond, flèche), traînée, pointeur laser,
//! indicateur du mode dessin et bulle de confirmation.

use crate::draw::{d2d, ellipse, Frame};
use crate::gpu::{Gpu, Surface};
use fx_core::{Color, DrawTool, Shape};
use windows::core::{w, Result};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows_numerics::Vector2;

fn v(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

// ---------------------------------------------------------------- Formes

pub unsafe fn draw_shapes(gpu: &Gpu, s: &Surface, f: &Frame) -> Result<()> {
    let (ox, oy) = (s.rect.left as f32, s.rect.top as f32);
    for shape in f.shapes {
        draw_shape(gpu, shape, ox, oy, f.scale)?;
    }
    Ok(())
}

/// Maj maintenue : carré / cercle (côtés égaux) ou flèche alignée sur 45°.
fn constrain(tool: DrawTool, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    match tool {
        DrawTool::Arrow => {
            let len = (dx * dx + dy * dy).sqrt();
            let step = std::f32::consts::FRAC_PI_4;
            let ang = (dy.atan2(dx) / step).round() * step;
            (a.0 + len * ang.cos(), a.1 + len * ang.sin())
        }
        _ => {
            let side = dx.abs().max(dy.abs());
            (a.0 + side * dx.signum(), a.1 + side * dy.signum())
        }
    }
}

unsafe fn draw_shape(gpu: &Gpu, sh: &Shape, ox: f32, oy: f32, scale: f32) -> Result<()> {
    let dc = &gpu.dc;
    let w = sh.width * scale;
    // Liseré sombre sous le trait : lisible sur fond clair comme sur fond foncé.
    let halo = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.28 };
    let color = d2d(sh.color);
    let a = (sh.points[0].0 - ox, sh.points[0].1 - oy);
    let last = *sh.points.last().unwrap();
    let mut b = (last.0 - ox, last.1 - oy);
    if sh.constrained && sh.tool != DrawTool::Pen {
        b = constrain(sh.tool, a, b);
    }

    match sh.tool {
        DrawTool::Pen => {
            let geom = pen_geometry(gpu, &sh.points, ox, oy)?;
            for (c, width) in [(halo, w + 2.0 * scale), (color, w)] {
                gpu.brush.SetColor(&c);
                dc.DrawGeometry(&geom, &gpu.brush, width, &gpu.round);
            }
        }
        DrawTool::Rect => {
            let r = D2D_RECT_F { left: a.0.min(b.0), top: a.1.min(b.1), right: a.0.max(b.0), bottom: a.1.max(b.1) };
            let rr = D2D1_ROUNDED_RECT { rect: r, radiusX: 3.0 * scale, radiusY: 3.0 * scale };
            for (c, width) in [(halo, w + 2.0 * scale), (color, w)] {
                gpu.brush.SetColor(&c);
                dc.DrawRoundedRectangle(&rr, &gpu.brush, width, &gpu.round);
            }
        }
        DrawTool::Ellipse => {
            let e = D2D1_ELLIPSE {
                point: v((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
                radiusX: (b.0 - a.0).abs() / 2.0,
                radiusY: (b.1 - a.1).abs() / 2.0,
            };
            for (c, width) in [(halo, w + 2.0 * scale), (color, w)] {
                gpu.brush.SetColor(&c);
                dc.DrawEllipse(&e, &gpu.brush, width, &gpu.round);
            }
        }
        DrawTool::Arrow => {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt().max(0.001);
            let (ux, uy) = (dx / len, dy / len);
            let head = (w * 3.6).max(14.0 * scale).min(len * 0.6);
            let half = head * 0.55;
            // Le trait s'arrête sous la pointe pour ne pas dépasser.
            let shaft_end = (b.0 - ux * head * 0.7, b.1 - uy * head * 0.7);
            let base = (b.0 - ux * head, b.1 - uy * head);
            let tri = [b, (base.0 - uy * half, base.1 + ux * half), (base.0 + uy * half, base.1 - ux * half)];
            let geom = polygon(gpu, &tri)?;
            for (c, width) in [(halo, w + 2.0 * scale), (color, w)] {
                gpu.brush.SetColor(&c);
                dc.DrawLine(v(a.0, a.1), v(shaft_end.0, shaft_end.1), &gpu.brush, width, &gpu.round);
            }
            gpu.brush.SetColor(&halo);
            dc.DrawGeometry(&geom, &gpu.brush, 2.0 * scale, &gpu.round);
            gpu.brush.SetColor(&color);
            dc.FillGeometry(&geom, &gpu.brush, None);
        }
    }
    Ok(())
}

/// Trait lissé : courbes de Bézier passant par le milieu de chaque segment.
unsafe fn pen_geometry(gpu: &Gpu, pts: &[(f32, f32)], ox: f32, oy: f32) -> Result<ID2D1PathGeometry> {
    let factory: &ID2D1Factory = &gpu.factory;
    let geom = factory.CreatePathGeometry()?;
    let sink = geom.Open()?;
    let p = |i: usize| (pts[i].0 - ox, pts[i].1 - oy);
    let first = p(0);
    sink.BeginFigure(v(first.0, first.1), D2D1_FIGURE_BEGIN_HOLLOW);
    if pts.len() < 3 {
        let l = p(pts.len() - 1);
        // Un simple point : petit segment pour que le trait rond soit visible.
        sink.AddLine(v(l.0 + 0.01, l.1));
    } else {
        for i in 1..pts.len() - 1 {
            let (c, n) = (p(i), p(i + 1));
            let mid = ((c.0 + n.0) / 2.0, (c.1 + n.1) / 2.0);
            sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: v(c.0, c.1), point2: v(mid.0, mid.1) });
        }
        let l = p(pts.len() - 1);
        sink.AddLine(v(l.0, l.1));
    }
    sink.EndFigure(D2D1_FIGURE_END_OPEN);
    sink.Close()?;
    Ok(geom)
}

unsafe fn polygon(gpu: &Gpu, pts: &[(f32, f32)]) -> Result<ID2D1PathGeometry> {
    let factory: &ID2D1Factory = &gpu.factory;
    let geom = factory.CreatePathGeometry()?;
    let sink = geom.Open()?;
    sink.BeginFigure(v(pts[0].0, pts[0].1), D2D1_FIGURE_BEGIN_FILLED);
    for p in &pts[1..] {
        sink.AddLine(v(p.0, p.1));
    }
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    sink.Close()?;
    Ok(geom)
}

// ---------------------------------------------------------------- Traînée et laser

pub unsafe fn draw_trail(gpu: &Gpu, s: &Surface, f: &Frame) {
    if f.trail.len() < 2 {
        return;
    }
    let (color, width) = if f.laser {
        (f.cfg.laser.color, f.cfg.laser.size * 1.3)
    } else {
        (f.cfg.trail.color, f.cfg.trail.width)
    };
    let (ox, oy) = (s.rect.left as f32, s.rect.top as f32);
    let w = width * f.scale;
    for pair in f.trail.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let life = b.2;
        if life <= 0.0 {
            continue;
        }
        // La queue s'amincit et s'efface ; la tête est pleine.
        gpu.brush.SetColor(&d2d(color.with_alpha(color.a * life * 0.85)));
        gpu.dc.DrawLine(v(a.0 - ox, a.1 - oy), v(b.0 - ox, b.1 - oy), &gpu.brush, w * (0.2 + 0.8 * life), &gpu.round);
    }
}

pub unsafe fn draw_laser(gpu: &Gpu, f: &Frame, cx: f32, cy: f32) -> Result<()> {
    let dc = &gpu.dc;
    let c = f.cfg.laser.color;
    let r = f.cfg.laser.size * f.scale;
    // Halo lumineux.
    let glow = r * 3.2;
    let stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: d2d(c.with_alpha(0.55)) },
        D2D1_GRADIENT_STOP { position: 0.35, color: d2d(c.with_alpha(0.22)) },
        D2D1_GRADIENT_STOP { position: 1.0, color: d2d(c.with_alpha(0.0)) },
    ];
    let rt: &ID2D1RenderTarget = dc;
    let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
        center: v(cx, cy),
        gradientOriginOffset: v(0.0, 0.0),
        radiusX: glow,
        radiusY: glow,
    };
    let brush = dc.CreateRadialGradientBrush(&props, None, &coll)?;
    dc.FillEllipse(&ellipse(cx, cy, glow), &brush);
    // Point plein et cœur clair.
    gpu.brush.SetColor(&d2d(c));
    dc.FillEllipse(&ellipse(cx, cy, r), &gpu.brush);
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.85 });
    dc.FillEllipse(&ellipse(cx, cy, r * 0.42), &gpu.brush);
    Ok(())
}

// ---------------------------------------------------------------- Mode dessin

/// Pastille de la couleur courante + petit pictogramme de l'outil, à côté du curseur.
pub unsafe fn draw_pointer(gpu: &Gpu, f: &Frame, cx: f32, cy: f32) {
    let dc = &gpu.dc;
    let k = f.scale;
    let col = d2d(f.draw_color);
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 });
    dc.FillEllipse(&ellipse(cx, cy, 5.5 * k), &gpu.brush);
    gpu.brush.SetColor(&col);
    dc.FillEllipse(&ellipse(cx, cy, 3.8 * k), &gpu.brush);

    // Pictogramme dans un petit badge en bas à droite du curseur.
    let (bx, by) = (cx + 22.0 * k, cy + 22.0 * k);
    let badge = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: bx - 11.0 * k, top: by - 11.0 * k, right: bx + 11.0 * k, bottom: by + 11.0 * k },
        radiusX: 5.0 * k,
        radiusY: 5.0 * k,
    };
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 0.08, g: 0.08, b: 0.1, a: 0.82 });
    dc.FillRoundedRectangle(&badge, &gpu.brush);
    gpu.brush.SetColor(&col);
    let s = 6.0 * k;
    let lw = 2.0 * k;
    match f.draw_tool {
        DrawTool::Pen => {
            dc.DrawLine(v(bx - s, by + s * 0.6), v(bx - s * 0.2, by - s * 0.3), &gpu.brush, lw, &gpu.round);
            dc.DrawLine(v(bx - s * 0.2, by - s * 0.3), v(bx + s, by + s * 0.2), &gpu.brush, lw, &gpu.round);
        }
        DrawTool::Rect => {
            let r = D2D_RECT_F { left: bx - s, top: by - s * 0.7, right: bx + s, bottom: by + s * 0.7 };
            dc.DrawRectangle(&r, &gpu.brush, lw, &gpu.round);
        }
        DrawTool::Ellipse => {
            let e = D2D1_ELLIPSE { point: v(bx, by), radiusX: s, radiusY: s * 0.72 };
            dc.DrawEllipse(&e, &gpu.brush, lw, &gpu.round);
        }
        DrawTool::Arrow => {
            dc.DrawLine(v(bx - s, by + s), v(bx + s, by - s), &gpu.brush, lw, &gpu.round);
            dc.DrawLine(v(bx + s, by - s), v(bx + s * 0.05, by - s), &gpu.brush, lw, &gpu.round);
            dc.DrawLine(v(bx + s, by - s), v(bx + s, by - s * 0.05), &gpu.brush, lw, &gpu.round);
        }
    }
}

// ---------------------------------------------------------------- Bulle de confirmation

pub unsafe fn draw_banner(gpu: &Gpu, s: &Surface, text: &str, alpha: f32) -> Result<()> {
    let k = s.scale;
    let format = gpu.dwrite.CreateTextFormat(
        w!("Segoe UI"),
        None,
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        15.0 * k,
        w!("fr-FR"),
    )?;
    let wide: Vec<u16> = text.encode_utf16().collect();
    let layout = gpu.dwrite.CreateTextLayout(&wide, &format, s.width() * 0.8, 200.0 * k)?;
    let mut m = DWRITE_TEXT_METRICS::default();
    layout.GetMetrics(&mut m)?;
    let (pad_x, pad_y) = (18.0 * k, 10.0 * k);
    let bw = m.width + 2.0 * pad_x;
    let bh = m.height + 2.0 * pad_y;
    let left = (s.width() - bw) / 2.0;
    let top = 28.0 * k;
    let rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left, top, right: left + bw, bottom: top + bh },
        radiusX: bh / 2.0,
        radiusY: bh / 2.0,
    };
    let dc = &gpu.dc;
    gpu.brush.SetColor(&d2d(Color::rgba8(20, 20, 24, 255).with_alpha(0.86 * alpha)));
    dc.FillRoundedRectangle(&rr, &gpu.brush);
    gpu.brush.SetColor(&d2d(Color::rgba8(255, 255, 255, 255).with_alpha(0.18 * alpha)));
    dc.DrawRoundedRectangle(&rr, &gpu.brush, 1.0 * k, None);
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: alpha });
    dc.DrawTextLayout(v(left + pad_x, top + pad_y), &layout, &gpu.brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
    Ok(())
}

// ---------------------------------------------------------------- Enregistrement

fn local(s: &Surface, r: [i32; 4]) -> D2D_RECT_F {
    let (ox, oy) = (s.rect.left as f32, s.rect.top as f32);
    D2D_RECT_F { left: r[0] as f32 - ox, top: r[1] as f32 - oy, right: r[2] as f32 - ox, bottom: r[3] as f32 - oy }
}

fn norm(r: [i32; 4]) -> [i32; 4] {
    [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])]
}

/// Cadre rouge autour de ce qui est enregistré (visible par le formateur seulement).
pub unsafe fn draw_rec_frame(gpu: &Gpu, s: &Surface, r: [i32; 4]) {
    let k = s.scale;
    let mut lr = local(s, r);
    let w = 3.0 * k;
    lr.left -= w / 2.0;
    lr.top -= w / 2.0;
    lr.right += w / 2.0;
    lr.bottom += w / 2.0;
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 0.93, g: 0.16, b: 0.2, a: 0.9 });
    gpu.dc.DrawRectangle(&lr, &gpu.brush, w, None);
}

/// Choix de la source : l'écran est voilé, sauf la fenêtre survolée ou la zone tracée.
pub unsafe fn draw_select(gpu: &Gpu, s: &Surface, f: &Frame, cx: f32, cy: f32) -> Result<()> {
    let dc = &gpu.dc;
    let k = s.scale;
    dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.42 }));
    if let Some(r) = f.select_rect {
        let lr = local(s, norm(r));
        dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
        gpu.brush.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 });
        dc.FillRectangle(&lr, &gpu.brush);
        dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        gpu.brush.SetColor(&D2D1_COLOR_F { r: 0.18, g: 0.49, b: 0.96, a: 1.0 });
        dc.DrawRectangle(&lr, &gpu.brush, 2.5 * k, None);
        // Dimensions de la zone, utiles pour viser un format.
        let [l, t, r2, b] = norm(r);
        let label = format!("{} × {}", r2 - l, b - t);
        draw_tag(gpu, s, &label, lr.left, lr.top - 26.0 * k)?;
    } else if f.select == 2 && s.contains(f.cursor.0, f.cursor.1) {
        // Zone : réticule pour viser le premier coin.
        gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.7 });
        dc.DrawLine(v(cx, 0.0), v(cx, s.height()), &gpu.brush, 1.0, None);
        dc.DrawLine(v(0.0, cy), v(s.width(), cy), &gpu.brush, 1.0, None);
    }
    Ok(())
}

/// Grand chiffre au centre de ce qui va être enregistré.
pub unsafe fn draw_countdown(gpu: &Gpu, s: &Surface, n: u32, r: [i32; 4]) -> Result<()> {
    let lr = local(s, r);
    let (cx, cy) = ((lr.left + lr.right) / 2.0, (lr.top + lr.bottom) / 2.0);
    if cx < 0.0 || cy < 0.0 || cx > s.width() || cy > s.height() {
        return Ok(());
    }
    let k = s.scale;
    let dc = &gpu.dc;
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 0.08, g: 0.08, b: 0.1, a: 0.78 });
    dc.FillEllipse(&ellipse(cx, cy, 70.0 * k), &gpu.brush);
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 0.93, g: 0.16, b: 0.2, a: 1.0 });
    dc.DrawEllipse(&ellipse(cx, cy, 70.0 * k), &gpu.brush, 4.0 * k, None);
    let format = gpu.dwrite.CreateTextFormat(
        w!("Segoe UI"),
        None,
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        72.0 * k,
        w!("fr-FR"),
    )?;
    format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
    format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
    let text: Vec<u16> = n.to_string().encode_utf16().collect();
    let box_ = D2D_RECT_F { left: cx - 70.0 * k, top: cy - 75.0 * k, right: cx + 70.0 * k, bottom: cy + 65.0 * k };
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
    dc.DrawText(&text, &format, &box_, &gpu.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
    Ok(())
}

/// Petite étiquette sombre (dimensions de la zone).
unsafe fn draw_tag(gpu: &Gpu, s: &Surface, text: &str, x: f32, y: f32) -> Result<()> {
    let k = s.scale;
    let format = gpu.dwrite.CreateTextFormat(
        w!("Segoe UI"),
        None,
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        12.0 * k,
        w!("fr-FR"),
    )?;
    let wide: Vec<u16> = text.encode_utf16().collect();
    let layout = gpu.dwrite.CreateTextLayout(&wide, &format, 400.0 * k, 40.0 * k)?;
    let mut m = DWRITE_TEXT_METRICS::default();
    layout.GetMetrics(&mut m)?;
    let y = y.max(4.0 * k);
    let rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: x, top: y, right: x + m.width + 14.0 * k, bottom: y + m.height + 6.0 * k },
        radiusX: 4.0 * k,
        radiusY: 4.0 * k,
    };
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 0.18, g: 0.49, b: 0.96, a: 1.0 });
    gpu.dc.FillRoundedRectangle(&rr, &gpu.brush);
    gpu.brush.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
    gpu.dc.DrawTextLayout(v(x + 7.0 * k, y + 3.0 * k), &layout, &gpu.brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
    Ok(())
}
