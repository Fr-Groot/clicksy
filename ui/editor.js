"use strict";
// Éditeur vidéo de Clicksy : aperçu (zone, zooms, coupes), frise, son, export.
// Le rendu final est fait par Clicksy (Rust) ; l'aperçu reproduit les mêmes calculs.

const TAURI = window.__TAURI__;
const $ = (id) => document.getElementById(id);

// Hors de Clicksy (aperçu dans un navigateur), un faux projet sans vidéo.
const MOCK_VIEW = {
  dir: "C:\\Users\\…\\Documents\\Clicksy\\Excel niveau 1\\2026-11-09 14h30\\Vidéo 14-31-05",
  name: "Vidéo 14-31-05", created: "2026-11-09 14-31-05", duration_ms: 42000,
  video_size: [1920, 1080], crop: [0, 0, 1920, 1080], has_system: true, has_mic: true, audio_offset_ms: 120,
  cursor_in_video: true,
  clicks: [3200, 4100, 12800, 21000, 21900, 33000].map((t, i) => ({ t, x: 0.2 + 0.1 * i, y: 0.3 + 0.05 * i, button: "L" })),
  cursor: [],
  edit: { trim_in: 0, trim_out: 42000, cuts: [[15000, 18000]], zooms: [
    { start: 2500, end: 5900, x: 0.25, y: 0.33, scale: 1.8 }, { start: 12100, end: 14600, x: 0.4, y: 0.4, scale: 1.8 },
    { start: 20300, end: 23700, x: 0.65, y: 0.5, scale: 1.8 }, { start: 32300, end: 34800, x: 0.7, y: 0.55, scale: 1.8 }],
    system_volume: 1, mic_volume: 1 },
  edit_saved: false, exports: ["Vidéo finale.mp4"],
};
const api = TAURI
  ? {
      projects: () => TAURI.core.invoke("editor_projects"),
      load: (dir) => TAURI.core.invoke("editor_load", { dir }),
      save: (dir, edit) => TAURI.core.invoke("editor_save", { dir, edit }),
      exportVideo: (dir, edit) => TAURI.core.invoke("editor_export", { dir, edit }),
      cancel: () => TAURI.core.invoke("editor_cancel"),
      reveal: (path) => TAURI.core.invoke("reveal", { path }),
      openFile: (path) => TAURI.core.invoke("open_file", { path }),
      lang: () => TAURI.core.invoke("ui_language"),
      src: (path) => TAURI.core.convertFileSrc(path),
      on: (ev, cb) => TAURI.event.listen(ev, (e) => cb(e.payload)),
    }
  : {
      projects: async () => [{ dir: MOCK_VIEW.dir, label: "Excel niveau 1 › 2026-11-09 14h30 › Vidéo 14-31-05" }],
      load: async () => structuredClone(MOCK_VIEW),
      save: async () => {},
      exportVideo: async () => { throw "Aperçu hors application"; },
      cancel: async () => {},
      reveal: async () => {},
      openFile: async () => {},
      lang: async () => new URLSearchParams(location.search).get("lang") || "fr",
      src: () => "",
      on: () => {},
    };

let view = null; // projet chargé
let edit = null; // réglages en cours
let selected = null; // { kind: "cut"|"zoom", i } ou { kind: "range", a, b }
let picking = false;
const video = $("video");
const audios = { system: $("a-system"), mic: $("a-mic") };

// ---------- Utilitaires ----------
const dur = () => (view ? view.duration_ms : 1);
function fmtTime(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}
const sep = (dir) => (dir.includes("\\") ? "\\" : "/");
let toastTimer;
function toast(msg) {
  const t = $("toast");
  t.textContent = msg;
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), 2600);
}

/** Passages gardés [début, fin] en ms (même calcul que l'export). */
function keptSegments() {
  const a = Math.min(edit.trim_in, dur());
  const b = Math.min(edit.trim_out, dur());
  const cuts = edit.cuts.map(([x, y]) => [Math.min(x, y), Math.max(x, y)]).sort((p, q) => p[0] - q[0]);
  const out = [];
  let cur = a;
  for (const [s, e] of cuts) {
    if (e <= cur || s >= b) continue;
    if (s > cur) out.push([cur, Math.min(s, b)]);
    cur = Math.max(cur, e);
  }
  if (cur < b) out.push([cur, b]);
  return out;
}

/** Grossissement et centre à l'instant t (ms), comme l'export. */
function zoomAt(t) {
  const EASE = 450;
  for (const z of edit.zooms) {
    if (t < z.start - EASE || t > z.end + EASE) continue;
    let k = t < z.start ? 1 - (z.start - t) / EASE : t > z.end ? 1 - (t - z.end) / EASE : 1;
    k = Math.min(1, Math.max(0, k));
    k = k * k * (3 - 2 * k);
    return { s: 1 + (Math.max(1, z.scale) - 1) * k, x: 0.5 + (z.x - 0.5) * k, y: 0.5 + (z.y - 0.5) * k };
  }
  return { s: 1, x: 0.5, y: 0.5 };
}

// ---------- Sauvegarde ----------
let saveTimer;
function changed() {
  renderTimeline();
  renderPanel();
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => api.save(view.dir, edit).catch((e) => toast(String(e))), 500);
}

// ---------- Chargement ----------
async function loadProject(dir) {
  try {
    view = await api.load(dir);
  } catch (e) {
    toast(String(e));
    return;
  }
  edit = view.edit;
  if (!edit.trim_out || edit.trim_out > view.duration_ms) edit.trim_out = view.duration_ms;
  selected = null;
  const s = sep(view.dir);
  video.src = api.src(view.dir + s + "screen.mp4");
  audios.system.src = view.has_system ? api.src(view.dir + s + "system.wav") : "";
  audios.mic.src = view.has_mic ? api.src(view.dir + s + "mic.wav") : "";
  $("project").value = view.dir;
  layout();
  renderTimeline();
  renderPanel();
  renderExports();
  if (!view.edit_saved && edit.zooms.length) toast(T("{} zooms proposés sur les clics", edit.zooms.length));
}

// ---------- Aperçu ----------
let k = 1; // pixels écran par pixel vidéo
function layout() {
  if (!view) return;
  const st = $("stage").getBoundingClientRect();
  const [cx, cy, cw, ch] = view.crop;
  k = Math.min((st.width - 2) / cw, (st.height - 2) / ch);
  const vp = $("viewport");
  vp.style.width = cw * k + "px";
  vp.style.height = ch * k + "px";
  const [vw, vh] = view.video_size;
  for (const el of [video, $("cursor")]) {
    el.style.width = vw * k + "px";
    el.style.height = vh * k + "px";
  }
  $("cursor").width = Math.round(vw * k);
  $("cursor").height = Math.round(vh * k);
  drawFrame();
}

function drawFrame() {
  if (!view) return;
  const t = video.currentTime * 1000;
  const [cx, cy, cw, ch] = view.crop;
  const z = zoomAt(t);
  const vw = cw / z.s, vh = ch / z.s;
  const centerX = Math.min(Math.max(cx + z.x * cw, cx + vw / 2), cx + cw - vw / 2);
  const centerY = Math.min(Math.max(cy + z.y * ch, cy + vh / 2), cy + ch - vh / 2);
  const ox = (centerX - vw / 2) * k, oy = (centerY - vh / 2) * k;
  $("layer").style.transform = `scale(${z.s}) translate(${-ox}px, ${-oy}px)`;
  drawCursor(t);
  // Tête de lecture et temps.
  $("playhead").style.left = (t / dur()) * 100 + "%";
  $("time").textContent = `${fmtTime(t)} / ${fmtTime(dur())}`;
}

/** Mode éditable : le curseur (absent de la vidéo) est dessiné par-dessus. */
function drawCursor(t) {
  const c = $("cursor");
  const g = c.getContext("2d");
  g.clearRect(0, 0, c.width, c.height);
  const tr = view.cursor;
  if (!tr.length) return;
  let lo = 0, hi = tr.length - 1;
  while (lo < hi) {
    const m = (lo + hi + 1) >> 1;
    if (tr[m][0] <= t) lo = m; else hi = m - 1;
  }
  const [, rx, ry] = tr[lo];
  if (rx < 0) return;
  const [cx, cy, cw, ch] = view.crop;
  const x = (cx + rx * cw) * k, y = (cy + ry * ch) * k;
  const s = Math.max(0.6, k * 1.4);
  g.save();
  g.translate(x, y);
  g.scale(s, s);
  g.beginPath();
  g.moveTo(0, 0); g.lineTo(0, 17); g.lineTo(4.5, 12.5); g.lineTo(7.5, 19); g.lineTo(10, 18); g.lineTo(7, 11.5); g.lineTo(12.5, 11.5);
  g.closePath();
  g.fillStyle = "#fff"; g.strokeStyle = "#000"; g.lineWidth = 1.2;
  g.fill(); g.stroke();
  g.restore();
}

// Lecture : les passages coupés sont sautés, le son suit l'image.
function syncAudio(force) {
  for (const [key, a] of Object.entries(audios)) {
    if (!a.src) continue;
    const vol = key === "system" ? edit.system_volume : edit.mic_volume;
    a.volume = Math.min(1, vol);
    const target = video.currentTime + view.audio_offset_ms / 1000;
    if (force || Math.abs(a.currentTime - target) > 0.15) a.currentTime = Math.max(0, target);
    if (video.paused) a.pause(); else a.play().catch(() => {});
  }
}

function tick() {
  if (view && !video.paused) {
    const t = video.currentTime * 1000;
    if (t >= edit.trim_out - 30) {
      video.pause();
      seek(edit.trim_in);
    } else if (t < edit.trim_in) {
      seek(edit.trim_in);
    } else {
      const cut = edit.cuts.find(([a, b]) => t >= Math.min(a, b) && t < Math.max(a, b));
      if (cut) seek(Math.max(cut[0], cut[1]));
    }
    syncAudio(false);
  }
  drawFrame();
  requestAnimationFrame(tick);
}

function seek(ms) {
  video.currentTime = Math.min(Math.max(0, ms), dur()) / 1000;
  syncAudio(true);
}

function togglePlay() {
  if (!view) return;
  if (video.paused) {
    const t = video.currentTime * 1000;
    if (t < edit.trim_in || t >= edit.trim_out - 50) seek(edit.trim_in);
    video.play().catch(() => {});
  } else video.pause();
  syncAudio(true);
}
video.addEventListener("play", () => ($("play").textContent = "❚❚"));
video.addEventListener("pause", () => { $("play").textContent = "▶"; syncAudio(true); });
video.addEventListener("loadedmetadata", layout);

// ---------- Frise ----------
const tl = $("timeline");
const pct = (ms) => (ms / dur()) * 100 + "%";
function msAt(clientX) {
  const r = tl.getBoundingClientRect();
  return Math.round(Math.min(1, Math.max(0, (clientX - r.left) / r.width)) * dur());
}

function renderTimeline() {
  if (!view) return;
  // Graduations : toutes les 5, 10, 30 ou 60 s selon la durée.
  const ruler = $("ruler");
  ruler.replaceChildren();
  const step = [1, 2, 5, 10, 15, 30, 60, 120, 300].find((s) => dur() / 1000 / s <= 14) || 600;
  for (let s = 0; s * 1000 <= dur(); s += step) {
    const d = document.createElement("div");
    d.className = "tick";
    d.style.left = pct(s * 1000);
    d.textContent = fmtTime(s * 1000);
    ruler.appendChild(d);
  }
  for (const c of view.clicks) {
    const d = document.createElement("div");
    d.className = "click-dot";
    d.style.left = pct(c.t);
    ruler.appendChild(d);
  }
  // Piste vidéo : passages gardés et coupés.
  const tv = $("track-video");
  tv.querySelectorAll(".seg").forEach((e) => e.remove());
  for (const [a, b] of keptSegments()) tv.appendChild(seg("keep", a, b));
  edit.cuts.forEach(([a, b], i) => {
    const s = seg("cut", Math.min(a, b), Math.max(a, b));
    s.title = T("Passage coupé : clique pour le sélectionner");
    if (selected?.kind === "cut" && selected.i === i) s.classList.add("selected");
    s.addEventListener("pointerdown", (e) => { e.stopPropagation(); select({ kind: "cut", i }); });
    tv.appendChild(s);
  });
  // Piste des zooms.
  const tz = $("track-zoom");
  tz.querySelectorAll(".seg").forEach((e) => e.remove());
  edit.zooms.forEach((z, i) => {
    const s = seg("zoom", z.start, z.end);
    s.textContent = Math.round(z.scale * 100) + " %";
    if (selected?.kind === "zoom" && selected.i === i) s.classList.add("selected");
    s.addEventListener("pointerdown", (e) => { e.stopPropagation(); select({ kind: "zoom", i }); dragZoom(e, i, "move"); });
    for (const side of ["l", "r"]) {
      const g = document.createElement("div");
      g.className = "grip " + side;
      g.addEventListener("pointerdown", (e) => { e.stopPropagation(); select({ kind: "zoom", i }); dragZoom(e, i, side); });
      s.appendChild(g);
    }
    tz.appendChild(s);
  });
  // Début et fin gardés.
  $("trim-in").style.left = pct(edit.trim_in);
  $("trim-out").style.left = pct(edit.trim_out);
  tl.querySelectorAll(".trimmed").forEach((e) => e.remove());
  for (const [a, b] of [[0, edit.trim_in], [edit.trim_out, dur()]]) {
    if (b <= a) continue;
    const d = document.createElement("div");
    d.className = "trimmed";
    d.style.left = pct(a);
    d.style.width = pct(b - a);
    tl.appendChild(d);
  }
  // Sélection d'un passage.
  const sel = $("selection");
  sel.hidden = selected?.kind !== "range";
  if (!sel.hidden) {
    sel.style.left = pct(selected.a);
    sel.style.width = pct(selected.b - selected.a);
  }
  const total = keptSegments().reduce((s, [a, b]) => s + b - a, 0);
  $("final").textContent = T("Vidéo finale : {}", fmtTime(total));
}

function seg(cls, a, b) {
  const d = document.createElement("div");
  d.className = "seg " + cls;
  d.style.left = pct(a);
  d.style.width = pct(Math.max(0, b - a));
  return d;
}

// Glisser dans la frise : sélectionne un passage ; simple clic : déplace la lecture.
tl.addEventListener("pointerdown", (e) => {
  if (!view || e.button !== 0) return;
  tl.setPointerCapture(e.pointerId);
  const start = msAt(e.clientX);
  seek(start);
  let moved = false;
  const move = (ev) => {
    const t = msAt(ev.clientX);
    if (Math.abs(t - start) > dur() / 200) moved = true;
    if (moved) {
      selected = { kind: "range", a: Math.min(start, t), b: Math.max(start, t) };
      renderTimeline();
      renderPanel();
    }
    seek(t);
  };
  const up = () => {
    tl.removeEventListener("pointermove", move);
    tl.removeEventListener("pointerup", up);
    if (!moved && selected?.kind === "range") select(null);
  };
  tl.addEventListener("pointermove", move);
  tl.addEventListener("pointerup", up);
});

function dragHandle(el, set) {
  el.addEventListener("pointerdown", (e) => {
    e.stopPropagation();
    el.setPointerCapture(e.pointerId);
    const move = (ev) => { set(msAt(ev.clientX)); renderTimeline(); seek(msAt(ev.clientX)); };
    const up = () => { el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); changed(); };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  });
}
dragHandle($("trim-in"), (t) => (edit.trim_in = Math.min(t, edit.trim_out - 500)));
dragHandle($("trim-out"), (t) => (edit.trim_out = Math.max(t, edit.trim_in + 500)));

function dragZoom(e, i, mode) {
  const el = e.currentTarget;
  el.setPointerCapture(e.pointerId);
  const z = edit.zooms[i];
  const t0 = msAt(e.clientX), s0 = z.start, e0 = z.end;
  const move = (ev) => {
    const d = msAt(ev.clientX) - t0;
    if (mode === "l") z.start = Math.min(Math.max(0, s0 + d), z.end - 300);
    else if (mode === "r") z.end = Math.max(Math.min(dur(), e0 + d), z.start + 300);
    else { const len = e0 - s0; z.start = Math.min(Math.max(0, s0 + d), dur() - len); z.end = z.start + len; }
    renderTimeline();
  };
  const up = () => { el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); changed(); };
  el.addEventListener("pointermove", move);
  el.addEventListener("pointerup", up);
}

// ---------- Panneau ----------
function select(s) {
  selected = s;
  picking = false;
  renderTimeline();
  renderPanel();
}

function renderPanel() {
  if (!view) return;
  const box = $("sel-box");
  box.hidden = !selected;
  $("zoom-ctl").hidden = true;
  $("pick-hint").hidden = !picking;
  $("stage").classList.toggle("picking", picking);
  if (selected) {
    const isRange = selected.kind === "range";
    $("sel-cut").hidden = !isRange;
    $("sel-zoom").hidden = !isRange;
    $("sel-remove").hidden = isRange;
    if (isRange) {
      $("sel-title").textContent = T("Passage sélectionné");
      $("sel-text").textContent = `${fmtTime(selected.a)} → ${fmtTime(selected.b)}`;
    } else if (selected.kind === "cut") {
      const [a, b] = edit.cuts[selected.i];
      $("sel-title").textContent = T("Passage coupé");
      $("sel-text").textContent = `${fmtTime(a)} → ${fmtTime(b)}`;
      $("sel-remove").textContent = T("Rétablir ce passage");
    } else {
      const z = edit.zooms[selected.i];
      $("sel-title").textContent = T("Zoom");
      $("sel-text").textContent = `${fmtTime(z.start)} → ${fmtTime(z.end)}`;
      $("sel-remove").textContent = T("Supprimer ce zoom");
      $("zoom-ctl").hidden = false;
      $("zoom-scale").value = z.scale;
      $("zoom-scale-out").textContent = Math.round(z.scale * 100) + " %";
    }
  }
  // Son.
  for (const [key, has] of [["system", view.has_system], ["mic", view.has_mic]]) {
    const row = $(`vol-${key}-row`);
    row.classList.toggle("disabled", !has);
    const input = $(`vol-${key}`);
    input.value = edit[`${key}_volume`];
    row.querySelector("output").textContent = Math.round(input.value * 100) + " %";
  }
  $("no-audio").hidden = view.has_system || view.has_mic;
}

$("sel-cut").addEventListener("click", () => {
  edit.cuts.push([selected.a, selected.b]);
  select(null);
  changed();
});
$("sel-zoom").addEventListener("click", () => {
  edit.zooms = edit.zooms.filter((z) => z.end <= selected.a || z.start >= selected.b);
  edit.zooms.push({ start: selected.a, end: selected.b, x: 0.5, y: 0.5, scale: 1.8 });
  edit.zooms.sort((a, b) => a.start - b.start);
  const i = edit.zooms.findIndex((z) => z.start === selected.a);
  select({ kind: "zoom", i });
  picking = true;
  renderPanel();
  changed();
});
$("sel-remove").addEventListener("click", removeSelected);
function removeSelected() {
  if (!selected || selected.kind === "range") return;
  if (selected.kind === "cut") edit.cuts.splice(selected.i, 1);
  else edit.zooms.splice(selected.i, 1);
  select(null);
  changed();
}
$("zoom-scale").addEventListener("input", (e) => {
  edit.zooms[selected.i].scale = Number(e.target.value);
  $("zoom-scale-out").textContent = Math.round(e.target.value * 100) + " %";
  renderTimeline();
});
$("zoom-scale").addEventListener("change", changed);
$("zoom-pick").addEventListener("click", () => {
  picking = !picking;
  const z = edit.zooms[selected.i];
  seek((z.start + z.end) / 2);
  renderPanel();
});
// Clic dans l'image : centre du zoom sélectionné.
$("viewport").addEventListener("pointerdown", (e) => {
  if (!picking || selected?.kind !== "zoom") return;
  const z = edit.zooms[selected.i];
  const r = $("viewport").getBoundingClientRect();
  // Position dans l'image affichée, ramenée à l'image sans zoom.
  const cur = zoomAt(video.currentTime * 1000);
  const fx = (e.clientX - r.left) / r.width, fy = (e.clientY - r.top) / r.height;
  z.x = Math.min(1, Math.max(0, cur.x + (fx - 0.5) / cur.s));
  z.y = Math.min(1, Math.max(0, cur.y + (fy - 0.5) / cur.s));
  picking = false;
  renderPanel();
  changed();
});
for (const key of ["system", "mic"]) {
  $(`vol-${key}`).addEventListener("input", (e) => {
    edit[`${key}_volume`] = Number(e.target.value);
    renderPanel();
    syncAudio(false);
  });
  $(`vol-${key}`).addEventListener("change", changed);
}
$("auto-zooms").addEventListener("click", () => {
  edit.zooms = autoZooms();
  select(null);
  changed();
  toast(T("{} zooms proposés sur les clics", edit.zooms.length));
});
$("clear-zooms").addEventListener("click", () => { edit.zooms = []; select(null); changed(); });

/** Même règle que Clicksy : clics gauches proches dans le temps et à l'écran → un zoom. */
function autoZooms() {
  const zooms = [];
  let group = [];
  const flush = () => {
    if (!group.length) return;
    const x = group.reduce((s, c) => s + c.x, 0) / group.length;
    const y = group.reduce((s, c) => s + c.y, 0) / group.length;
    const start = Math.max(0, group[0].t - 700), end = Math.min(dur(), group[group.length - 1].t + 1800);
    if (end > start + 500) zooms.push({ start, end, x, y, scale: 1.8 });
    group = [];
  };
  for (const c of view.clicks.filter((c) => c.button === "L")) {
    const l = group[group.length - 1];
    if (!(l && c.t - l.t < 2500 && Math.abs(c.x - l.x) < 0.3 && Math.abs(c.y - l.y) < 0.3)) flush();
    group.push(c);
  }
  flush();
  for (let i = 1; i < zooms.length; i++) {
    if (zooms[i].start < zooms[i - 1].end) {
      const mid = Math.round((zooms[i].start + zooms[i - 1].end) / 2);
      zooms[i - 1].end = mid;
      zooms[i].start = mid;
    }
  }
  return zooms;
}

// ---------- Export ----------
let exporting = false;
$("export").addEventListener("click", async () => {
  if (!view || exporting) return;
  video.pause();
  try {
    await api.exportVideo(view.dir, edit);
    exporting = true;
    $("progress").hidden = false;
    $("bar").style.width = "0";
    $("cancel").hidden = false;
    $("export").disabled = true;
    $("export").textContent = T("Export en cours…");
  } catch (e) {
    toast(String(e));
  }
});
$("cancel").addEventListener("click", () => api.cancel());
api.on("export-progress", (p) => { if (view && p.dir === view.dir) $("bar").style.width = Math.round(p.progress * 100) + "%"; });
api.on("export-done", async (d) => {
  exporting = false;
  $("progress").hidden = true;
  $("cancel").hidden = true;
  $("export").disabled = false;
  $("export").textContent = T("Exporter la vidéo");
  if (d.error) { toast(d.error); return; }
  toast(T("Vidéo exportée"));
  if (view && d.dir === view.dir) {
    const name = d.path.split(sep(d.path)).pop();
    if (!view.exports.includes(name)) view.exports.push(name);
    renderExports();
  }
});

function renderExports() {
  const ul = $("exports");
  ul.replaceChildren();
  for (const name of view.exports) {
    const path = view.dir + sep(view.dir) + name;
    const li = document.createElement("li");
    const span = document.createElement("span");
    span.textContent = name;
    span.title = path;
    const play = Object.assign(document.createElement("button"), { className: "btn", textContent: T("Lire") });
    play.addEventListener("click", () => api.openFile(path));
    const show = Object.assign(document.createElement("button"), { className: "btn", textContent: T("Afficher") });
    show.addEventListener("click", () => api.reveal(path));
    li.append(span, play, show);
    ul.appendChild(li);
  }
}

// ---------- Divers ----------
$("play").addEventListener("click", togglePlay);
$("reveal").addEventListener("click", () => view && api.reveal(view.dir));
$("project").addEventListener("change", (e) => loadProject(e.target.value));
window.addEventListener("resize", layout);
document.addEventListener("keydown", (e) => {
  if (e.target.tagName === "SELECT" || e.target.tagName === "INPUT") return;
  if (e.code === "Space") { e.preventDefault(); togglePlay(); }
  else if (e.key === "ArrowLeft") seek(video.currentTime * 1000 - 1000);
  else if (e.key === "ArrowRight") seek(video.currentTime * 1000 + 1000);
  else if (e.key === "Delete") removeSelected();
  else if (e.key === "Escape") select(null);
});

(async function main() {
  LANG = await api.lang();
  translatePage();
  const list = await api.projects();
  const sel = $("project");
  for (const p of list) sel.appendChild(Object.assign(document.createElement("option"), { value: p.dir, textContent: p.label }));
  const wanted = new URLSearchParams(location.search).get("dir");
  if (!list.length && !wanted) {
    $("main").hidden = true;
    $("empty").hidden = false;
    $("export").disabled = true;
    return;
  }
  if (wanted && !list.some((p) => p.dir === wanted)) {
    sel.prepend(Object.assign(document.createElement("option"), { value: wanted, textContent: wanted }));
  }
  await loadProject(wanted || list[0].dir);
  api.on("editor-open", (dir) => {
    if (![...sel.options].some((o) => o.value === dir)) sel.prepend(Object.assign(document.createElement("option"), { value: dir, textContent: dir }));
    loadProject(dir);
  });
  requestAnimationFrame(tick);
})();
