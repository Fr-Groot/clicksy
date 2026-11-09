//! Guide pas à pas : pendant l'enregistrement du guide, chaque clic devient une étape
//! (capture de l'écran + description automatique), chaque raccourci aussi. À la fin, une page
//! HTML autonome (`guide.html`) est créée dans le dossier de la session : modifiable,
//! imprimable et enregistrable en PDF depuis le navigateur.
//!
//! Le texte tapé n'est jamais noté : une étape « Saisir le texte voulu » le remplace.

use fx_bus::MouseButton;
use fx_core::{t, tf, Shared};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

#[derive(Debug, Clone, Serialize)]
struct Step {
    /// « click », « right », « double », « keys » ou « text ».
    kind: String,
    text: String,
    /// Image de l'écran (nom de fichier) et position du clic (0 à 1).
    image: Option<String>,
    x: f32,
    y: f32,
    w: i32,
    h: i32,
    /// Pour réécrire le texte d'un clic devenu double-clic.
    #[serde(skip)]
    name: String,
    #[serde(skip)]
    control: i32,
    #[serde(skip)]
    window: String,
}

struct Job {
    n: usize,
    kind: &'static str,
    pt: POINT,
    shot: Option<(i32, i32, Vec<u8>, [i32; 2])>,
    window: String,
}

struct Guide {
    dir: PathBuf,
    steps: Arc<Mutex<Vec<Option<Step>>>>,
    tx: Option<Sender<Job>>,
    worker: Option<JoinHandle<()>>,
    last_click: Option<(Instant, i32, i32, MouseButton)>,
    /// Numéro de la dernière étape de clic, et étapes devenues des double-clics.
    last_n: usize,
    doubles: Vec<usize>,
    typed: bool,
    title: String,
}

static GUIDE: Mutex<Option<Guide>> = Mutex::new(None);

impl Step {
    fn empty() -> Step {
        Step { kind: String::new(), text: String::new(), image: None, x: 0.0, y: 0.0, w: 0, h: 0, name: String::new(), control: 0, window: String::new() }
    }
}

pub fn active() -> bool {
    GUIDE.lock().unwrap().is_some()
}

/// Bouton « Guide » ou raccourci : démarre ou termine le guide.
pub fn toggle(shared: &Shared) {
    if active() {
        finish(shared);
    } else {
        start(shared);
    }
}

fn start(shared: &Shared) {
    let base = match crate::session::dir(shared) {
        Ok(d) => d,
        Err(e) => {
            shared.banner(tf("Guide impossible : {}", &[&e]));
            return;
        }
    };
    let stamp = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let dir = base.join(format!("{} {:02}-{:02}-{:02}", t("Guide"), stamp.wHour, stamp.wMinute, stamp.wSecond));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        shared.banner(tf("Guide impossible : {}", &[&e.to_string()]));
        return;
    }
    let steps: Arc<Mutex<Vec<Option<Step>>>> = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = channel::<Job>();
    let (wdir, wsteps) = (dir.clone(), steps.clone());
    // Le travail lent (description par UI Automation, écriture PNG) se fait hors du moteur.
    let worker = std::thread::spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let uia: Option<IUIAutomation> = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok() };
        for job in rx {
            let step = build_step(&wdir, &job, uia.as_ref());
            let mut s = wsteps.lock().unwrap();
            if s.len() <= job.n {
                s.resize(job.n + 1, None);
            }
            s[job.n] = Some(step);
        }
    });
    *GUIDE.lock().unwrap() = Some(Guide {
        dir,
        steps,
        tx: Some(tx),
        worker: Some(worker),
        last_click: None,
        last_n: 0,
        doubles: Vec::new(),
        typed: false,
        title: String::new(),
    });
    shared.modes.guide.store(true, Ordering::Release);
    shared.banner(t("Guide pas à pas : chaque clic devient une étape · Guide pour terminer"));
}

fn window_title() -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(GetForegroundWindow(), &mut buf);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

/// Clic pendant le guide : capture immédiate de l'écran (avant que l'appli ne réagisse).
pub fn on_click(shared: &Shared, button: MouseButton, x: i32, y: i32) {
    let mut g = GUIDE.lock().unwrap();
    let Some(guide) = g.as_mut() else { return };
    if !matches!(button, MouseButton::Left | MouseButton::Right) {
        return;
    }
    // Double-clic : même endroit, moins d'une demi-seconde → l'étape précédente devient « double ».
    if let Some((at, px, py, b)) = guide.last_click {
        if b == button && button == MouseButton::Left && at.elapsed().as_millis() < 500 && (px - x).abs() < 6 && (py - y).abs() < 6 {
            // L'étape précédente peut être encore en préparation : on la corrige à la fin.
            guide.doubles.push(guide.last_n);
            guide.last_click = None;
            return;
        }
    }
    guide.last_click = Some((Instant::now(), x, y, button));
    flush_typed(guide);
    let pt = POINT { x, y };
    let shot = unsafe { crate::capture::grab_with_origin(pt) }.ok();
    let window = window_title();
    if guide.title.is_empty() {
        guide.title = window.clone();
    }
    let n = next_index(guide);
    guide.last_n = n;
    let kind = if button == MouseButton::Right { "right" } else { "click" };
    if let Some(tx) = &guide.tx {
        let _ = tx.send(Job { n, kind, pt, shot, window });
    }
    drop(g);
    shared.banner(tf("Guide : étape {}", &[&(n + 1).to_string()]));
}

/// Raccourci clavier pendant le guide (« Ctrl + S ») : une étape sans image.
pub fn on_combo(parts: &[String]) {
    let mut g = GUIDE.lock().unwrap();
    let Some(guide) = g.as_mut() else { return };
    flush_typed(guide);
    let n = next_index(guide);
    let text = tf("Appuyer sur {}", &[&parts.join(" + ")]);
    guide.steps.lock().unwrap().resize(n + 1, None);
    guide.steps.lock().unwrap()[n] = Some(Step { kind: "keys".into(), text, ..Step::empty() });
}

/// Texte tapé pendant le guide : noté, sans son contenu.
pub fn on_typed() {
    if let Some(guide) = GUIDE.lock().unwrap().as_mut() {
        guide.typed = true;
    }
}

fn flush_typed(guide: &mut Guide) {
    if guide.typed {
        guide.typed = false;
        let n = next_index(guide);
        let mut s = guide.steps.lock().unwrap();
        s.resize(n + 1, None);
        s[n] = Some(Step { kind: "text".into(), text: t("Saisir le texte voulu").into(), ..Step::empty() });
    }
}

/// Réserve le numéro de l'étape suivante.
fn next_index(guide: &Guide) -> usize {
    let mut s = guide.steps.lock().unwrap();
    let n = s.len();
    s.push(None);
    n
}

/// Description d'une étape de clic : nom et type de l'élément sous la souris.
fn build_step(dir: &std::path::Path, job: &Job, uia: Option<&IUIAutomation>) -> Step {
    let (name, control) = uia.and_then(|u| unsafe { describe(u, job.pt) }).unwrap_or_default();
    let mut image = None;
    let (mut x, mut y, mut w, mut h) = (0.0, 0.0, 0, 0);
    if let Some((iw, ih, pixels, origin)) = &job.shot {
        let file = format!("{} {:02}.png", t("étape"), job.n + 1);
        if unsafe { crate::capture::save_png(&dir.join(&file), *iw as u32, *ih as u32, pixels) }.is_ok() {
            image = Some(file);
            (w, h) = (*iw, *ih);
            x = (job.pt.x - origin[0]) as f32 / *iw as f32;
            y = (job.pt.y - origin[1]) as f32 / *ih as f32;
        }
    }
    let text = click_text(job.kind, &name, control, &job.window);
    Step { kind: job.kind.into(), text, image, x, y, w, h, name, control, window: job.window.clone() }
}

/// Nom (tronqué) et type de contrôle de l'élément sous le point.
unsafe fn describe(uia: &IUIAutomation, pt: POINT) -> Option<(String, i32)> {
    let el = uia.ElementFromPoint(pt).ok()?;
    let name = el.CurrentName().map(|b| b.to_string()).unwrap_or_default();
    let control = el.CurrentControlType().map(|c| c.0).unwrap_or(0);
    let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = if name.chars().count() > 60 { name.chars().take(57).collect::<String>() + "…" } else { name };
    Some((name, control))
}

fn click_text(kind: &str, name: &str, control: i32, window: &str) -> String {
    if name.is_empty() {
        let base = match kind {
            "right" => t("Faire un clic droit à l'endroit indiqué"),
            "double" => t("Double-cliquer à l'endroit indiqué"),
            _ => t("Cliquer à l'endroit indiqué"),
        };
        return if window.is_empty() { base.into() } else { format!("{base} {}", tf("dans « {} »", &[window])) };
    }
    // Types de contrôle UI Automation (UIA_*ControlTypeId).
    let tpl = match (kind, control) {
        ("right", _) => "Faire un clic droit sur « {} »",
        ("double", _) => "Double-cliquer sur « {} »",
        (_, 50000) => "Cliquer sur le bouton « {} »",
        (_, 50011) => "Cliquer sur le menu « {} »",
        (_, 50019) => "Cliquer sur l'onglet « {} »",
        (_, 50005) => "Cliquer sur le lien « {} »",
        (_, 50002) => "Cocher la case « {} »",
        (_, 50013) => "Choisir l'option « {} »",
        (_, 50004) => "Cliquer dans le champ « {} »",
        (_, 50003) => "Ouvrir la liste « {} »",
        (_, 50007) | (_, 50024) => "Sélectionner « {} »",
        _ => "Cliquer sur « {} »",
    };
    tf(tpl, &[name])
}

/// Termine le guide : écrit `guide.html` et l'ouvre dans le navigateur.
fn finish(shared: &Shared) {
    let Some(mut guide) = GUIDE.lock().unwrap().take() else { return };
    shared.modes.guide.store(false, Ordering::Release);
    flush_typed(&mut guide);
    drop(guide.tx.take());
    if let Some(w) = guide.worker.take() {
        let _ = w.join();
    }
    let mut all = guide.steps.lock().unwrap().clone();
    for &n in &guide.doubles {
        if let Some(Some(s)) = all.get_mut(n) {
            s.kind = "double".into();
            s.text = click_text("double", &s.name, s.control, &s.window);
        }
    }
    let steps: Vec<Step> = all.into_iter().flatten().collect();
    if steps.is_empty() {
        let _ = std::fs::remove_dir(&guide.dir);
        shared.banner(t("Guide annulé : aucune étape"));
        return;
    }
    let html = render_html(&guide.title, &steps);
    let path = guide.dir.join(format!("{}.html", t("Guide")));
    match std::fs::write(&path, html) {
        Ok(()) => {
            shared.banner(tf("Guide terminé : {} étapes", &[&steps.len().to_string()]));
            let _ = std::process::Command::new("explorer.exe").arg(&path).spawn();
        }
        Err(e) => shared.banner(tf("Guide impossible : {}", &[&e.to_string()])),
    }
}

fn render_html(title: &str, steps: &[Step]) -> String {
    let now = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let date = format!("{:02}/{:02}/{:04}", now.wDay, now.wMonth, now.wYear);
    let title = if title.is_empty() { t("Guide pas à pas").to_string() } else { format!("{} – {title}", t("Guide pas à pas")) };
    let data = serde_json::to_string(steps).unwrap_or_else(|_| "[]".into()).replace("</", "<\\/");
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    TEMPLATE
        .replace("{{lang}}", fx_core::i18n::code())
        .replace("{{title}}", &esc(&title))
        .replace("{{date}}", &date)
        .replace("{{print}}", t("Imprimer ou enregistrer en PDF"))
        .replace("{{tip}}", t("Tu peux modifier les textes avant d'imprimer. Clique sur une image pour voir l'écran entier."))
        .replace("{{step}}", t("Étape"))
        .replace("{{made}}", t("Créé avec Clicksy le"))
        .replace("{{data}}", &data)
}

const TEMPLATE: &str = r#"<!doctype html>
<html lang="{{lang}}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{{title}}</title>
<style>
  :root { --accent: #0a64d8; --text: #1b1b1f; --muted: #5f6070; --border: #e1e1e6; }
  * { box-sizing: border-box; }
  body { margin: 0; background: #f3f3f5; color: var(--text); font: 15px/1.5 "Segoe UI", system-ui, sans-serif; }
  .page { max-width: 900px; margin: 0 auto; padding: 32px 20px 60px; }
  header { display: flex; gap: 16px; align-items: flex-start; justify-content: space-between; flex-wrap: wrap; }
  h1 { font-size: 26px; margin: 0 0 4px; outline: none; }
  .meta, .tip { color: var(--muted); margin: 0; }
  .tip { margin: 14px 0 24px; padding: 10px 14px; background: #fff4ce; border-radius: 8px; color: #6a4a00; }
  button { font: 600 14px "Segoe UI", sans-serif; padding: 9px 16px; border-radius: 8px; border: none; background: var(--accent); color: #fff; cursor: pointer; }
  .step { background: #fff; border: 1px solid var(--border); border-radius: 12px; padding: 16px; margin: 0 0 16px; break-inside: avoid; }
  .head { display: flex; gap: 12px; align-items: center; }
  .num { flex: none; width: 32px; height: 32px; border-radius: 50%; background: var(--accent); color: #fff; font-weight: 700; display: grid; place-items: center; }
  .text { flex: 1; font-size: 16px; font-weight: 600; outline: none; padding: 2px 4px; border-radius: 4px; }
  .text:focus { background: #eef4ff; }
  .shot { position: relative; margin-top: 12px; border-radius: 8px; overflow: hidden; border: 1px solid var(--border); cursor: zoom-in; background-repeat: no-repeat; background-color: #222; }
  .shot.full { cursor: zoom-out; }
  .mark { position: absolute; width: 46px; height: 46px; margin: -23px 0 0 -23px; border: 4px solid #e5202e; border-radius: 50%; box-shadow: 0 0 0 3px rgba(255,255,255,.85); pointer-events: none; }
  footer { color: var(--muted); font-size: 13px; text-align: center; margin-top: 30px; }
  @media print {
    body { background: #fff; }
    .page { padding: 0; }
    button, .tip { display: none; }
    .step { border-color: #ccc; }
  }
</style>
</head>
<body>
<div class="page">
  <header>
    <div>
      <h1 contenteditable="true" spellcheck="false">{{title}}</h1>
      <p class="meta">{{date}}</p>
    </div>
    <button onclick="window.print()">{{print}}</button>
  </header>
  <p class="tip">{{tip}}</p>
  <main id="steps"></main>
  <footer>{{made}} {{date}}</footer>
</div>
<script>
const STEPS = {{data}};
const root = document.getElementById("steps");
STEPS.forEach((s, i) => {
  const box = document.createElement("section");
  box.className = "step";
  box.innerHTML = '<div class="head"><span class="num"></span><div class="text" contenteditable="true"></div></div>';
  box.querySelector(".num").textContent = i + 1;
  box.querySelector(".num").title = "{{step}} " + (i + 1);
  box.querySelector(".text").textContent = s.text;
  if (s.image) {
    const shot = document.createElement("div");
    shot.className = "shot";
    const mark = document.createElement("div");
    mark.className = "mark";
    shot.appendChild(mark);
    // Vue rapprochée autour du clic (40 % de la largeur de l'écran), ou écran entier.
    const show = (full) => {
      const r = s.w / s.h;
      const cw = full ? 1 : Math.min(1, Math.max(0.3, 900 / s.w * 0.9));
      const ch = cw;
      const x0 = full ? 0 : Math.min(Math.max(s.x - cw / 2, 0), 1 - cw);
      const y0 = full ? 0 : Math.min(Math.max(s.y - ch / 2, 0), 1 - ch);
      shot.style.aspectRatio = r;
      shot.style.backgroundImage = 'url("' + encodeURI(s.image) + '")';
      shot.style.backgroundSize = (100 / cw) + "% " + (100 / ch) + "%";
      shot.style.backgroundPosition = (cw >= 1 ? 0 : x0 / (1 - cw) * 100) + "% " + (ch >= 1 ? 0 : y0 / (1 - ch) * 100) + "%";
      mark.style.left = ((s.x - x0) / cw * 100) + "%";
      mark.style.top = ((s.y - y0) / ch * 100) + "%";
      shot.classList.toggle("full", full);
    };
    let full = false;
    show(full);
    shot.addEventListener("click", () => show(full = !full));
    box.appendChild(shot);
  }
  root.appendChild(box);
});
</script>
</body>
</html>
"#;
