//! Éditeur : lecture d'un projet, zooms automatiques et export de la vidéo finale.
//!
//! L'export relit `screen.mp4` (décodeur Media Foundation), applique la zone, les passages
//! coupés, les zooms et, en mode éditable, redessine le curseur ; il mixe le son du PC et le
//! micro, puis encode le tout en H.264 + AAC (`windows-capture`, encodeur de Windows).
//! La sortie est à cadence fixe : les zooms restent fluides même quand l'écran ne bouge pas.

use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::HSTRING;
use windows::Win32::Media::MediaFoundation::*;
use windows_capture::encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder};

/// Réglages de l'éditeur, enregistrés dans `edit.json` du dossier projet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Edit {
    /// Début et fin gardés (ms, temps de la vidéo).
    pub trim_in: u64,
    pub trim_out: u64,
    /// Passages supprimés [début, fin] (ms).
    pub cuts: Vec<[u64; 2]>,
    pub zooms: Vec<Zoom>,
    pub system_volume: f32,
    pub mic_volume: f32,
}

impl Default for Edit {
    fn default() -> Self {
        Edit { trim_in: 0, trim_out: u64::MAX, cuts: Vec::new(), zooms: Vec::new(), system_volume: 1.0, mic_volume: 1.0 }
    }
}

/// Zoom sur une partie de l'image entre `start` et `end` (ms). `x`, `y` : centre, de 0 à 1
/// dans l'image finale ; `scale` : grossissement (1,5 = 150 %).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Zoom {
    pub start: u64,
    pub end: u64,
    pub x: f32,
    pub y: f32,
    pub scale: f32,
}

/// Clic, en temps vidéo (ms) et en position relative (0 à 1) dans l'image finale.
#[derive(Debug, Clone, Serialize)]
pub struct Click {
    pub t: u64,
    pub x: f32,
    pub y: f32,
    pub button: String,
}

/// Ce que l'éditeur a besoin de savoir d'un projet.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectView {
    pub dir: String,
    pub name: String,
    pub created: String,
    pub duration_ms: u64,
    /// Taille de la vidéo source et zone gardée [x, y, largeur, hauteur] dans cette vidéo.
    pub video_size: [u32; 2],
    pub crop: [u32; 4],
    pub has_system: bool,
    pub has_mic: bool,
    /// Décalage (ms) entre le début des pistes son et la première image de la vidéo.
    pub audio_offset_ms: f64,
    pub cursor_in_video: bool,
    pub clicks: Vec<Click>,
    /// Curseur (mode éditable) : [t ms, x, y] en position relative dans l'image finale, -1 = caché.
    pub cursor: Vec<[f32; 3]>,
    pub edit: Edit,
    pub edit_saved: bool,
    pub exports: Vec<String>,
}

type R<T> = Result<T, String>;

fn read_project(dir: &Path) -> R<Project> {
    let text = std::fs::read_to_string(dir.join("project.json")).map_err(|e| format!("project.json : {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("project.json : {e}"))
}

/// Zone gardée dans la vidéo (dimensions paires, comme l'exige H.264).
fn crop_of(p: &Project) -> [u32; 4] {
    let (w, h) = (p.video.width.max(2), p.video.height.max(2));
    if let Some(r) = p.source.region {
        let c = p.source.capture_rect;
        let x = (r[0] - c[0]).clamp(0, w as i32 - 2) as u32;
        let y = (r[1] - c[1]).clamp(0, h as i32 - 2) as u32;
        let cw = ((r[2] - r[0]) as u32).min(w - x) & !1;
        let ch = ((r[3] - r[1]) as u32).min(h - y) & !1;
        return [x, y, cw.max(2), ch.max(2)];
    }
    [0, 0, w & !1, h & !1]
}

/// Temps (ms) entre l'instant t0 du journal et la première image de la vidéo.
fn video_offset_ms(p: &Project) -> f64 {
    (p.video.first_frame_100ns - p.t0_100ns) as f64 / 10_000.0
}

/// Lit le journal : clics et trajectoire du curseur, en temps vidéo et en position relative.
fn read_journal(dir: &Path, p: &Project, crop: [u32; 4]) -> (Vec<Click>, Vec<[f32; 3]>) {
    let mut clicks = Vec::new();
    let mut cursor = Vec::new();
    let Ok(text) = std::fs::read_to_string(dir.join(&p.events)) else { return (clicks, cursor) };
    let off = video_offset_ms(p);
    let mut origin = (p.source.capture_rect[0], p.source.capture_rect[1]);
    let rel = |x: i32, y: i32, o: (i32, i32)| {
        (((x - o.0) as f32 - crop[0] as f32) / crop[2] as f32, ((y - o.1) as f32 - crop[1] as f32) / crop[3] as f32)
    };
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let t = v["t"].as_f64().unwrap_or(0.0) - off;
        let (x, y) = (v["x"].as_i64().unwrap_or(0) as i32, v["y"].as_i64().unwrap_or(0) as i32);
        match v["k"].as_str() {
            Some("r") => {
                if let Some(r) = v["rect"].as_array() {
                    origin = (r[0].as_i64().unwrap_or(0) as i32, r[1].as_i64().unwrap_or(0) as i32);
                }
            }
            Some("d") if t >= 0.0 => {
                let (rx, ry) = rel(x, y, origin);
                if (0.0..=1.0).contains(&rx) && (0.0..=1.0).contains(&ry) {
                    clicks.push(Click { t: t as u64, x: rx, y: ry, button: v["b"].as_str().unwrap_or("L").into() });
                }
            }
            Some("c") => {
                let (rx, ry) = rel(x, y, origin);
                let visible = v["v"].as_bool().unwrap_or(true);
                let id = v["id"].as_u64().unwrap_or(0) as f32;
                // On garde l'identifiant de forme dans la partie entière de t négative ? Non :
                // la forme est retrouvée à l'export ; l'aperçu n'a besoin que de la position.
                let _ = id;
                cursor.push([t.max(0.0) as f32, if visible { rx } else { -1.0 }, ry]);
            }
            _ => {}
        }
    }
    (clicks, cursor)
}

/// Zooms automatiques : les clics proches dans le temps forment un zoom centré sur eux.
pub fn auto_zooms(clicks: &[Click], duration_ms: u64) -> Vec<Zoom> {
    let mut zooms: Vec<Zoom> = Vec::new();
    let mut group: Vec<&Click> = Vec::new();
    let flush = |group: &mut Vec<&Click>, zooms: &mut Vec<Zoom>| {
        if group.is_empty() {
            return;
        }
        let n = group.len() as f32;
        let x = group.iter().map(|c| c.x).sum::<f32>() / n;
        let y = group.iter().map(|c| c.y).sum::<f32>() / n;
        let start = group[0].t.saturating_sub(700);
        let end = (group[group.len() - 1].t + 1800).min(duration_ms);
        if end > start + 500 {
            zooms.push(Zoom { start, end, x, y, scale: 1.8 });
        }
        group.clear();
    };
    for c in clicks.iter().filter(|c| c.button == "L") {
        // Même groupe : moins de 2,5 s depuis le clic précédent et assez près à l'écran.
        let close = group.last().is_some_and(|l| c.t - l.t < 2500 && (c.x - l.x).abs() < 0.3 && (c.y - l.y).abs() < 0.3);
        if !close {
            flush(&mut group, &mut zooms);
        }
        group.push(c);
    }
    flush(&mut group, &mut zooms);
    // Deux zooms qui se chevauchent : on les sépare.
    for i in 1..zooms.len() {
        if zooms[i].start < zooms[i - 1].end {
            let mid = (zooms[i].start + zooms[i - 1].end) / 2;
            zooms[i - 1].end = mid;
            zooms[i].start = mid;
        }
    }
    zooms
}

/// Liste des fichiers exportés d'un projet.
fn exports_of(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_lowercase().ends_with(".mp4") && n != "screen.mp4")
        .collect();
    v.sort();
    v
}

pub fn load(dir: &Path) -> R<ProjectView> {
    let p = read_project(dir)?;
    let crop = crop_of(&p);
    let (clicks, cursor) = read_journal(dir, &p, crop);
    let fps = p.video.fps.max(1) as u64;
    // Durée : celle du projet moins le délai avant la première image.
    let duration_ms = (p.duration_ms as f64 - video_offset_ms(&p)).max(p.video.frames as f64 * 1000.0 / fps as f64) as u64;
    let saved = std::fs::read_to_string(dir.join("edit.json")).ok().and_then(|t| serde_json::from_str::<Edit>(&t).ok());
    let edit_saved = saved.is_some();
    let mut edit = saved.unwrap_or_else(|| Edit { zooms: auto_zooms(&clicks, duration_ms), ..Edit::default() });
    edit.trim_out = edit.trim_out.min(duration_ms);
    Ok(ProjectView {
        dir: dir.display().to_string(),
        name: dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        created: p.created.clone(),
        duration_ms,
        video_size: [p.video.width, p.video.height],
        crop,
        has_system: p.audio.system.as_ref().is_some_and(|f| dir.join(f).exists()),
        has_mic: p.audio.mic.as_ref().is_some_and(|f| dir.join(f).exists()),
        audio_offset_ms: video_offset_ms(&p),
        cursor_in_video: p.cursor_in_video,
        clicks,
        cursor: if p.cursor_in_video { Vec::new() } else { cursor },
        edit,
        edit_saved,
        exports: exports_of(dir),
    })
}

pub fn save(dir: &Path, edit: &Edit) -> R<()> {
    let text = serde_json::to_string_pretty(edit).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("edit.json"), text).map_err(|e| format!("edit.json : {e}"))
}

/// Projets d'enregistrement trouvés sous ces dossiers (3 niveaux au plus), du plus récent au plus ancien.
pub fn find_projects(roots: &[PathBuf]) -> Vec<(PathBuf, std::time::SystemTime)> {
    let mut out = Vec::new();
    fn walk(d: &Path, depth: u32, out: &mut Vec<(PathBuf, std::time::SystemTime)>) {
        let pj = d.join("project.json");
        if let Ok(m) = std::fs::metadata(&pj) {
            out.push((d.to_path_buf(), m.modified().unwrap_or(std::time::UNIX_EPOCH)));
            return;
        }
        if depth == 0 {
            return;
        }
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                walk(&e.path(), depth - 1, out);
            }
        }
    }
    for r in roots {
        walk(r, 3, &mut out);
    }
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

// ---------------------------------------------------------------- Temps

/// Passages gardés [début, fin] (ms, temps de la vidéo source), dans l'ordre.
fn kept_segments(e: &Edit, duration: u64) -> Vec<(u64, u64)> {
    let (a, b) = (e.trim_in.min(duration), e.trim_out.min(duration));
    let mut cuts: Vec<(u64, u64)> = e.cuts.iter().map(|c| (c[0].min(c[1]), c[0].max(c[1]))).collect();
    cuts.sort();
    let mut out = Vec::new();
    let mut cur = a;
    for (s, t) in cuts {
        if t <= cur || s >= b {
            continue;
        }
        if s > cur {
            out.push((cur, s.min(b)));
        }
        cur = cur.max(t);
    }
    if cur < b {
        out.push((cur, b));
    }
    out
}

/// Temps de sortie (s) → temps source (s).
fn to_source(segs: &[(u64, u64)], t_out: f64) -> f64 {
    let mut acc = 0.0;
    for &(s, e) in segs {
        let len = (e - s) as f64 / 1000.0;
        if t_out < acc + len {
            return s as f64 / 1000.0 + (t_out - acc);
        }
        acc += len;
    }
    segs.last().map(|s| s.1 as f64 / 1000.0).unwrap_or(0.0)
}

/// Grossissement et centre (relatifs) à l'instant `t` (ms source), avec transitions douces.
fn zoom_at(zooms: &[Zoom], t: f64) -> (f32, f32, f32) {
    const EASE: f64 = 450.0;
    for z in zooms {
        let (s, e) = (z.start as f64, z.end as f64);
        if t < s - EASE || t > e + EASE {
            continue;
        }
        // Montée avant le début, descente après la fin (courbe en S).
        let k = if t < s {
            1.0 - (s - t) / EASE
        } else if t > e {
            1.0 - (t - e) / EASE
        } else {
            1.0
        }
        .clamp(0.0, 1.0);
        let k = (k * k * (3.0 - 2.0 * k)) as f32;
        let scale = 1.0 + (z.scale.max(1.0) - 1.0) * k;
        return (scale, 0.5 + (z.x - 0.5) * k, 0.5 + (z.y - 0.5) * k);
    }
    (1.0, 0.5, 0.5)
}

// ---------------------------------------------------------------- Son

/// Piste WAV flottante stéréo 48 kHz chargée en mémoire.
struct Wav {
    samples: Vec<f32>,
}

impl Wav {
    fn load(path: &Path) -> Option<Wav> {
        let data = std::fs::read(path).ok()?;
        if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
            return None;
        }
        let mut i = 12;
        while i + 8 <= data.len() {
            let id = &data[i..i + 4];
            let len = u32::from_le_bytes(data[i + 4..i + 8].try_into().ok()?) as usize;
            if id == b"data" {
                let end = (i + 8 + len).min(data.len());
                let samples = data[i + 8..end].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
                return Some(Wav { samples });
            }
            i += 8 + len + (len & 1);
        }
        None
    }

    /// Échantillon stéréo n° `frame` (silence au-delà).
    fn at(&self, frame: i64) -> (f32, f32) {
        if frame < 0 {
            return (0.0, 0.0);
        }
        let i = frame as usize * 2;
        match (self.samples.get(i), self.samples.get(i + 1)) {
            (Some(l), Some(r)) => (*l, *r),
            _ => (0.0, 0.0),
        }
    }
}

// ---------------------------------------------------------------- Décodage

/// Lecteur Media Foundation qui rend des images BGRA de haut en bas.
struct Decoder {
    reader: IMFSourceReader,
    width: u32,
    height: u32,
    stride: usize,
}

const MF_VERSION: u32 = 0x0002_0070;
const FIRST_VIDEO: u32 = 0xFFFF_FFFC; // MF_SOURCE_READER_FIRST_VIDEO_STREAM

impl Decoder {
    unsafe fn open(path: &Path) -> windows::core::Result<Decoder> {
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.unwrap();
        // Conversion YUV → RGB faite par Media Foundation.
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attrs)?;
        let mt = MFCreateMediaType()?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(FIRST_VIDEO, None, &mt)?;
        let cur = reader.GetCurrentMediaType(FIRST_VIDEO)?;
        let size = cur.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let (width, height) = ((size >> 32) as u32, size as u32);
        let stride = cur.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|s| (s as i32).unsigned_abs() as usize).unwrap_or(width as usize * 4);
        Ok(Decoder { reader, width, height, stride })
    }

    /// Image suivante : (temps en secondes, pixels BGRA de haut en bas, largeur × hauteur × 4).
    unsafe fn next(&mut self) -> windows::core::Result<Option<(f64, Vec<u8>)>> {
        loop {
            let mut flags = 0u32;
            let mut ts = 0i64;
            let mut sample: Option<IMFSample> = None;
            self.reader.ReadSample(FIRST_VIDEO, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut sample))?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
            let Some(sample) = sample else { continue };
            let buf = sample.ConvertToContiguousBuffer()?;
            let mut ptr = std::ptr::null_mut();
            let mut len = 0u32;
            buf.Lock(&mut ptr, None, Some(&mut len))?;
            let src = std::slice::from_raw_parts(ptr, len as usize);
            let row = self.width as usize * 4;
            let mut out = vec![0u8; row * self.height as usize];
            for y in 0..self.height as usize {
                let s = y * self.stride;
                if s + row <= src.len() {
                    out[y * row..(y + 1) * row].copy_from_slice(&src[s..s + row]);
                }
            }
            let _ = buf.Unlock();
            // RGB32 : l'octet alpha n'est pas défini.
            for px in out.chunks_exact_mut(4) {
                px[3] = 255;
            }
            return Ok(Some((ts as f64 / 10_000_000.0, out)));
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
        }
    }
}

// ---------------------------------------------------------------- Curseur (mode éditable)

struct CursorSprite {
    w: u32,
    h: u32,
    hot: (i32, i32),
    bgra: Vec<u8>,
}

/// Formes de curseur enregistrées (`cursors/cursors.json` + PNG).
fn load_cursors(dir: &Path) -> std::collections::HashMap<u64, CursorSprite> {
    let mut map = std::collections::HashMap::new();
    let Ok(text) = std::fs::read_to_string(dir.join("cursors").join("cursors.json")) else { return map };
    let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else { return map };
    for c in list {
        let file = c["file"].as_str().unwrap_or_default();
        if let Some((w, h, bgra)) = crate::png::load(&dir.join("cursors").join(file)) {
            let hot = (c["hot_x"].as_i64().unwrap_or(0) as i32, c["hot_y"].as_i64().unwrap_or(0) as i32);
            map.insert(c["id"].as_u64().unwrap_or(0), CursorSprite { w, h, hot, bgra });
        }
    }
    map
}

/// Trajectoire du curseur en temps vidéo (ms) et en pixels de la vidéo source.
fn cursor_track(dir: &Path, p: &Project) -> Vec<(f64, i32, i32, u64, bool)> {
    let mut out = Vec::new();
    let Ok(text) = std::fs::read_to_string(dir.join(&p.events)) else { return out };
    let off = video_offset_ms(p);
    let mut origin = (p.source.capture_rect[0], p.source.capture_rect[1]);
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        match v["k"].as_str() {
            Some("r") => {
                if let Some(r) = v["rect"].as_array() {
                    origin = (r[0].as_i64().unwrap_or(0) as i32, r[1].as_i64().unwrap_or(0) as i32);
                }
            }
            Some("c") => out.push((
                v["t"].as_f64().unwrap_or(0.0) - off,
                v["x"].as_i64().unwrap_or(0) as i32 - origin.0,
                v["y"].as_i64().unwrap_or(0) as i32 - origin.1,
                v["id"].as_u64().unwrap_or(0),
                v["v"].as_bool().unwrap_or(true),
            )),
            _ => {}
        }
    }
    out
}

/// Pose le curseur (alpha non prémultiplié) sur l'image source.
fn blend_cursor(img: &mut [u8], w: u32, h: u32, s: &CursorSprite, x: i32, y: i32) {
    let (x0, y0) = (x - s.hot.0, y - s.hot.1);
    for cy in 0..s.h as i32 {
        let py = y0 + cy;
        if py < 0 || py >= h as i32 {
            continue;
        }
        for cx in 0..s.w as i32 {
            let px = x0 + cx;
            if px < 0 || px >= w as i32 {
                continue;
            }
            let si = ((cy as u32 * s.w + cx as u32) * 4) as usize;
            let a = s.bgra[si + 3] as u32;
            if a == 0 {
                continue;
            }
            let di = ((py as u32 * w + px as u32) * 4) as usize;
            for c in 0..3 {
                img[di + c] = ((s.bgra[si + c] as u32 * a + img[di + c] as u32 * (255 - a)) / 255) as u8;
            }
        }
    }
}

// ---------------------------------------------------------------- Mise à l'échelle

/// Copie le rectangle `view` (pixels source, flottants) dans `out` (ow × oh), en bilinéaire,
/// lignes de bas en haut (format attendu par l'encodeur). Réparti sur plusieurs cœurs.
fn resample(src: &[u8], sw: u32, sh: u32, view: [f32; 4], out: &mut [u8], ow: u32, oh: u32) {
    let [vx, vy, vw, vh] = view;
    let row = ow as usize * 4;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
    let chunk_rows = (oh as usize).div_ceil(threads);
    std::thread::scope(|scope| {
        for (ci, chunk) in out.chunks_mut(chunk_rows * row).enumerate() {
            scope.spawn(move || {
                for (ri, line) in chunk.chunks_exact_mut(row).enumerate() {
                    let oy = ci * chunk_rows + ri;
                    // Ligne n° oy du tampon = ligne oh-1-oy de l'image (de bas en haut).
                    let iy = oh as usize - 1 - oy;
                    let fy = (vy + (iy as f32 + 0.5) * vh / oh as f32 - 0.5).clamp(0.0, sh as f32 - 1.0);
                    let y0 = fy.floor() as usize;
                    let y1 = (y0 + 1).min(sh as usize - 1);
                    let wy = fy - y0 as f32;
                    for ox in 0..ow as usize {
                        let fx = (vx + (ox as f32 + 0.5) * vw / ow as f32 - 0.5).clamp(0.0, sw as f32 - 1.0);
                        let x0 = fx.floor() as usize;
                        let x1 = (x0 + 1).min(sw as usize - 1);
                        let wx = fx - x0 as f32;
                        let p = |x: usize, y: usize, c: usize| src[(y * sw as usize + x) * 4 + c] as f32;
                        for c in 0..3 {
                            let top = p(x0, y0, c) * (1.0 - wx) + p(x1, y0, c) * wx;
                            let bot = p(x0, y1, c) * (1.0 - wx) + p(x1, y1, c) * wx;
                            line[ox * 4 + c] = (top * (1.0 - wy) + bot * wy + 0.5) as u8;
                        }
                        line[ox * 4 + 3] = 255;
                    }
                }
            });
        }
    });
}

// ---------------------------------------------------------------- Export

/// Exporte la vidéo finale dans `out`. `progress` reçoit l'avancement de 0 à 1.
pub fn export(dir: &Path, edit: &Edit, out: &Path, cancel: &AtomicBool, mut progress: impl FnMut(f32)) -> R<()> {
    let p = read_project(dir)?;
    let view = load(dir)?;
    let segs = kept_segments(edit, view.duration_ms);
    let total: f64 = segs.iter().map(|(s, e)| (e - s) as f64 / 1000.0).sum();
    if total < 0.2 {
        return Err("rien à exporter : toute la vidéo est coupée".into());
    }
    let crop = view.crop;
    let (ow, oh) = (crop[2], crop[3]);
    let fps = p.video.fps.clamp(15, 60);

    // Son : PC + micro mixés, alignés sur la vidéo.
    let system = p.audio.system.as_ref().and_then(|f| Wav::load(&dir.join(f)));
    let mic = p.audio.mic.as_ref().and_then(|f| Wav::load(&dir.join(f)));
    let has_audio = system.is_some() || mic.is_some();
    let rate = crate::audio::RATE as f64;
    let audio_offset = view.audio_offset_ms / 1000.0;

    // Curseur à redessiner (mode éditable).
    let sprites = if p.cursor_in_video { Default::default() } else { load_cursors(dir) };
    let track = if p.cursor_in_video { Vec::new() } else { cursor_track(dir, &p) };
    let mut track_i = 0usize;

    let bitrate = ((ow as u64 * oh as u64 * fps as u64) / 10).clamp(4_000_000, 40_000_000) as u32;
    let mut enc = VideoEncoder::new(
        VideoSettingsBuilder::new(ow, oh).frame_rate(fps).bitrate(bitrate),
        AudioSettingsBuilder::default().disabled(!has_audio),
        ContainerSettingsBuilder::default(),
        out,
    )
    .map_err(|e| format!("encodeur : {e}"))?;

    let mut dec = unsafe { Decoder::open(&dir.join(&p.video.file)) }.map_err(|e| format!("lecture de la vidéo : {e}"))?;
    let (sw, sh) = (dec.width, dec.height);
    let mut cur: Option<(f64, Vec<u8>)> = unsafe { dec.next() }.map_err(|e| e.to_string())?;
    let mut next: Option<(f64, Vec<u8>)> = unsafe { dec.next() }.map_err(|e| e.to_string())?;
    let mut frame_buf = vec![0u8; (ow * oh * 4) as usize];
    let mut audio_sent: u64 = 0;
    let n_frames = (total * fps as f64).ceil() as u64;

    for i in 0..n_frames {
        if cancel.load(Ordering::Acquire) {
            let _ = enc.finish();
            let _ = std::fs::remove_file(out);
            return Err("export annulé".into());
        }
        let t_out = i as f64 / fps as f64;
        let t_src = to_source(&segs, t_out);
        // Avance jusqu'à la dernière image dont l'horodatage est passé.
        while next.as_ref().is_some_and(|n| n.0 <= t_src) {
            cur = next.take();
            next = unsafe { dec.next() }.map_err(|e| e.to_string())?;
        }
        let Some((_, img)) = cur.as_ref() else { break };
        let mut img_ref: std::borrow::Cow<[u8]> = std::borrow::Cow::Borrowed(img);

        // Curseur redessiné.
        if !track.is_empty() {
            let tms = t_src * 1000.0;
            if track_i >= track.len() || track[track_i.min(track.len() - 1)].0 > tms {
                track_i = 0;
            }
            while track_i + 1 < track.len() && track[track_i + 1].0 <= tms {
                track_i += 1;
            }
            let (_, x, y, id, visible) = track[track_i];
            if visible {
                if let Some(s) = sprites.get(&id) {
                    let mut owned = img.clone();
                    blend_cursor(&mut owned, sw, sh, s, x, y);
                    img_ref = std::borrow::Cow::Owned(owned);
                }
            }
        }

        // Zone visible : zone gardée, réduite par le zoom autour de son centre.
        let (scale, zx, zy) = zoom_at(&edit.zooms, t_src * 1000.0);
        let (vw, vh) = (crop[2] as f32 / scale, crop[3] as f32 / scale);
        let cx = (crop[0] as f32 + zx * crop[2] as f32).clamp(crop[0] as f32 + vw / 2.0, (crop[0] + crop[2]) as f32 - vw / 2.0);
        let cy = (crop[1] as f32 + zy * crop[3] as f32).clamp(crop[1] as f32 + vh / 2.0, (crop[1] + crop[3]) as f32 - vh / 2.0);
        resample(&img_ref, sw, sh, [cx - vw / 2.0, cy - vh / 2.0, vw, vh], &mut frame_buf, ow, oh);
        enc.send_frame_buffer(&frame_buf, (t_out * 10_000_000.0) as i64 + 1).map_err(|e| format!("encodeur : {e}"))?;

        // Son jusqu'à la fin de cette image.
        if has_audio {
            let until = (((i + 1) as f64 / fps as f64) * rate) as u64;
            let mut pcm = Vec::with_capacity(((until - audio_sent) * 4) as usize);
            for k in audio_sent..until {
                let src_t = to_source(&segs, k as f64 / rate) + audio_offset;
                let f = (src_t * rate) as i64;
                let mut l = 0.0;
                let mut r = 0.0;
                if let Some(w) = &system {
                    let (a, b) = w.at(f);
                    l += a * edit.system_volume;
                    r += b * edit.system_volume;
                }
                if let Some(w) = &mic {
                    let (a, b) = w.at(f);
                    l += a * edit.mic_volume;
                    r += b * edit.mic_volume;
                }
                for v in [l, r] {
                    pcm.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
                }
            }
            audio_sent = until;
            enc.send_audio_buffer(&pcm, 0).map_err(|e| format!("encodeur (son) : {e}"))?;
        }
        if i % 10 == 0 {
            progress(i as f32 / n_frames as f32);
        }
    }
    enc.finish().map_err(|e| format!("finalisation : {e}"))?;
    progress(1.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoupe() {
        let e = Edit { trim_in: 1000, trim_out: 9000, cuts: vec![[3000, 4000], [6000, 7000]], ..Edit::default() };
        let s = kept_segments(&e, 10_000);
        assert_eq!(s, vec![(1000, 3000), (4000, 6000), (7000, 9000)]);
        assert!((to_source(&s, 0.0) - 1.0).abs() < 1e-9);
        assert!((to_source(&s, 2.5) - 4.5).abs() < 1e-9);
        assert!((to_source(&s, 4.0) - 7.0).abs() < 1e-9);
    }

    #[test]
    fn zooms_auto() {
        let c = |t, x| Click { t, x, y: 0.5, button: "L".into() };
        let z = auto_zooms(&[c(1000, 0.2), c(2000, 0.25), c(9000, 0.8)], 20_000);
        assert_eq!(z.len(), 2);
        assert!(z[0].x > 0.2 && z[0].x < 0.25);
        assert_eq!(zoom_at(&z, 5000.0).0, 1.0);
        assert!((zoom_at(&z, 1500.0).0 - 1.8).abs() < 1e-6);
    }
}
