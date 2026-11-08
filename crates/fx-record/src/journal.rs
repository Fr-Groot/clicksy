//! Journal `events.jsonl` : une ligne JSON par événement, `t` en millisecondes depuis le début.
//!
//! - `{"t":12.5,"k":"c","x":..,"y":..,"id":3,"v":true}` : position / forme / visibilité du curseur ;
//! - `{"t":..,"k":"d"|"u","b":"L"|"R"|"M","x":..,"y":..}` : bouton enfoncé / relâché ;
//! - `{"t":..,"k":"w","d":120,"x":..,"y":..}` : molette ;
//! - `{"t":..,"k":"r","rect":[g,h,d,b]}` : position de la fenêtre enregistrée (source fenêtre).
//!
//! Coordonnées en pixels physiques du bureau. Les touches du clavier ne sont **jamais** écrites.

use crate::clock::now_100ns;
use crate::project::{window_rect, SourceInfo};
use crossbeam_channel::Receiver;
use fx_bus::{Event, EventKind, MouseButton};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{GetCursorInfo, CURSORINFO, CURSOR_SHOWING, HCURSOR};

#[derive(Serialize)]
struct CursorShape {
    id: u32,
    file: String,
    hot_x: i32,
    hot_y: i32,
    width: u32,
    height: u32,
}

pub fn record(dir: &Path, t0: i64, source: &SourceInfo, rx: Receiver<Event>, stop: &AtomicBool) -> Result<(), String> {
    let file = File::create(dir.join("events.jsonl")).map_err(|e| format!("events.jsonl : {e}"))?;
    let mut out = BufWriter::new(file);
    // Conversion horloge du bus (µs depuis son démarrage) → horloge commune.
    let offset = now_100ns() - fx_bus::now_us() as i64 * 10;
    let ms = |t100: i64| ((t100 - t0) as f64 / 10_000.0 * 10.0).round() / 10.0;

    let mut shapes: HashMap<isize, u32> = HashMap::new();
    let mut shape_list: Vec<CursorShape> = Vec::new();
    let mut last: Option<(i32, i32, u32, bool)> = None;
    let mut last_rect: Option<[i32; 4]> = None;
    let mut tick = 0u32;

    let write = |out: &mut BufWriter<File>, v: serde_json::Value| -> Result<(), String> {
        serde_json::to_writer(&mut *out, &v).map_err(|e| e.to_string())?;
        out.write_all(b"\n").map_err(|e| e.to_string())
    };

    while !stop.load(Ordering::Acquire) {
        // Clics et molette, horodatés par les hooks.
        while let Ok(ev) = rx.try_recv() {
            let t = ms(ev.t_us as i64 * 10 + offset);
            if t < 0.0 {
                continue;
            }
            let b = |b: MouseButton| match b {
                MouseButton::Left => "L",
                MouseButton::Right => "R",
                MouseButton::Middle => "M",
                MouseButton::X1 => "X1",
                MouseButton::X2 => "X2",
            };
            let v = match ev.kind {
                EventKind::MouseDown { button, x, y } => serde_json::json!({"t": t, "k": "d", "b": b(button), "x": x, "y": y}),
                EventKind::MouseUp { button, x, y } => serde_json::json!({"t": t, "k": "u", "b": b(button), "x": x, "y": y}),
                EventKind::Wheel { delta, horizontal, x, y } => {
                    serde_json::json!({"t": t, "k": "w", "d": delta, "h": horizontal, "x": x, "y": y})
                }
                _ => continue,
            };
            write(&mut out, v)?;
        }

        // Position et forme du curseur (échantillonnées toutes les 8 ms).
        let mut ci = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
        if unsafe { GetCursorInfo(&mut ci) }.is_ok() {
            let visible = ci.flags.0 & CURSOR_SHOWING.0 != 0;
            let key = ci.hCursor.0 as isize;
            let id = match shapes.get(&key) {
                Some(id) => *id,
                None => {
                    let id = shape_list.len() as u32 + 1;
                    shapes.insert(key, id);
                    if let Some(s) = save_cursor(dir, ci.hCursor, id) {
                        shape_list.push(s);
                    }
                    id
                }
            };
            let POINT { x, y } = ci.ptScreenPos;
            let now = (x, y, id, visible);
            if last != Some(now) {
                last = Some(now);
                write(&mut out, serde_json::json!({"t": ms(now_100ns()), "k": "c", "x": x, "y": y, "id": id, "v": visible}))?;
            }
        }

        // Fenêtre enregistrée : on suit ses déplacements (toutes les 100 ms).
        tick += 1;
        if source.hwnd != 0 && tick % 12 == 0 {
            if let Some(r) = window_rect(source.hwnd) {
                let r = [r.left, r.top, r.right, r.bottom];
                if last_rect != Some(r) {
                    last_rect = Some(r);
                    write(&mut out, serde_json::json!({"t": ms(now_100ns()), "k": "r", "rect": r}))?;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    out.flush().map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&shape_list).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("cursors").join("cursors.json"), text).map_err(|e| e.to_string())
}

/// Enregistre l'image d'une forme de curseur (PNG avec transparence) et son point actif.
fn save_cursor(dir: &Path, cursor: HCURSOR, id: u32) -> Option<CursorShape> {
    let (w, h, hot, bgra) = crate::png::cursor_bitmap(cursor)?;
    let file = format!("{id}.png");
    crate::png::save(&dir.join("cursors").join(&file), w, h, &bgra).ok()?;
    Some(CursorShape { id, file, hot_x: hot.0, hot_y: hot.1, width: w, height: h })
}
