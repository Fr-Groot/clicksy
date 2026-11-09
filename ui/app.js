"use strict";

// Hors de Clicksy (aperçu dans un navigateur), on simule le backend.
const TAURI = window.__TAURI__;
const MOCK = {
  version: 1, enabled: true,
  spotlight: { mode: "Off", shape: "Circle", radius: 140, band_size: 160, dim: 0.55, dim_color: "#000000", feather: 24 },
  ring: { enabled: true, radius: 26, thickness: 4, color: "#FFC400", opacity: 0.9, pulse: false, pulse_period_ms: 1400 },
  clicks: { enabled: true, left_color: "#1E90FF", right_color: "#FF3B5C", middle_color: "#2ECC71", radius: 42, thickness: 4,
            duration_ms: 450, show_held: true, sound: false, sound_volume: 0.5 },
  draw: { colors: ["#FF3B30", "#FFCC00", "#1E90FF"], width: 5, default_tool: "Pen", clear_on_exit: true, save_board: true },
  trail: { enabled: false, color: "#FFC400", duration_ms: 350, width: 6 },
  laser: { color: "#FF2222", size: 7, duration_ms: 600 },
  show_banner: true,
  toolbar: { visible: true, hide_from_capture: true, x: null, y: null },
  capture: { folder: "", copy_to_clipboard: true },
  record: { system_audio: true, mic: true, fps: 30, countdown: true, folder: "", open_folder_after: true, burn_effects: true },
  keys: { enabled: false, all_keys: false, mouse: true, position: "Center", duration_ms: 1600, size: 22 },
  session: { enabled: true, folder: "", formation: "Excel niveau 1", formations: ["Excel niveau 1", "Teams pour débutants"] },
  hotkeys: { toggle_all: "Ctrl+Alt+Maj+A", cycle_spotlight: "Ctrl+Alt+Maj+S", toggle_ring: "Ctrl+Alt+Maj+R", toggle_clicks: "Ctrl+Alt+Maj+C",
             toggle_draw: "Ctrl+Alt+Maj+D", toggle_laser: "Ctrl+Alt+Maj+L", toggle_trail: "Ctrl+Alt+Maj+T",
             toggle_toolbar: "Ctrl+Alt+Maj+O", cycle_board: "Ctrl+Alt+Maj+W", screenshot: "Ctrl+Alt+Maj+P",
             toggle_record: "Ctrl+Alt+Maj+E", toggle_keys: "Ctrl+Alt+Maj+K", next_profile: "Ctrl+Alt+Maj+Y" },
  language: "Auto",
  active_profile: "Tuto",
};
// Champs qu'un profil enregistre (même liste que ProfileSettings côté Rust).
const PROFILE_FIELDS = ["enabled", "show_banner", "spotlight", "ring", "clicks", "draw", "trail", "laser", "keys", "record"];
const snapshot = (c) => structuredClone(Object.fromEntries(PROFILE_FIELDS.map((k) => [k, c[k]])));
MOCK.profiles = ["Tuto", "Classe virtuelle", "Stream", "Démo logiciel"].map((name) => ({ name, settings: snapshot(MOCK) }));
const api = TAURI
  ? {
      get: () => TAURI.core.invoke("get_config"),
      defaults: () => TAURI.core.invoke("default_config"),
      set: (config, save) => TAURI.core.invoke("set_config", { config, save }),
      openDir: () => TAURI.core.invoke("open_data_dir"),
      dataDir: () => TAURI.core.invoke("data_dir"),
      action: (name) => TAURI.core.invoke("run_action", { name }),
      session: () => TAURI.core.invoke("session_info"),
      setFormation: (name) => TAURI.core.invoke("set_formation", { name }),
      lang: () => TAURI.core.invoke("ui_language"),
      onChange: (cb) => TAURI.event.listen("config-changed", (e) => cb(e.payload)),
    }
  : {
      get: async () => structuredClone(MOCK),
      defaults: async () => structuredClone(MOCK),
      set: async (c) => c,
      openDir: async () => {},
      dataDir: async () => "(aperçu hors application)",
      action: async () => {},
      session: async () => ["", "C:\\Users\\…\\Documents\\Clicksy"],
      setFormation: async (name) => { MOCK.session.formation = name; return structuredClone(MOCK); },
      lang: async () => (new URLSearchParams(location.search).get("lang") || "fr"),
      onChange: () => {},
    };

const RING_PRESETS = ["#FFC400", "#FF3B5C", "#1E90FF", "#2ECC71", "#FFFFFF", "#111111"];

let cfg = null;
let dragging = false;

// ---------- Accès par chemin ("ring.radius") ----------
const getPath = (o, p) => p.split(".").reduce((a, k) => (a == null ? a : a[k]), o);
function setPath(o, p, v) {
  const keys = p.split(".");
  const last = keys.pop();
  keys.reduce((a, k) => a[k], o)[last] = v;
}

// ---------- Envoi au moteur ----------
let pending = false;
function pushLive() {
  // Aperçu en direct, une fois par image au plus.
  if (pending) return;
  pending = true;
  requestAnimationFrame(() => {
    pending = false;
    api.set(cfg, false).catch(showError);
  });
}
function pushSave() {
  api.set(cfg, true).then((applied) => { cfg = applied; render(); }).catch(showError);
}

// ---------- Affichage ----------
function fmt(input, v) {
  if (input.hasAttribute("data-percent")) return Math.round(v * 100) + " %";
  const unit = input.dataset.unit;
  if (unit === "ms") return (v >= 1000 ? (LANG === "en" ? (v / 1000).toFixed(1) : (v / 1000).toFixed(1).replace(".", ",")) + " s" : v + " ms");
  return Number(v).toLocaleString(LANG === "en" ? "en-GB" : "fr-FR", { maximumFractionDigits: 1 }) + " px";
}

function render() {
  if (!cfg) return;
  document.querySelectorAll("[data-path]").forEach((el) => {
    const v = getPath(cfg, el.dataset.path);
    if (el.classList.contains("segmented")) {
      el.querySelectorAll("button").forEach((b) => b.classList.toggle("on", b.dataset.value === String(v)));
    } else if (el.type === "checkbox") {
      el.checked = !!v;
    } else if (el.type === "range") {
      if (!dragging) el.value = v;
      el.nextElementSibling.textContent = fmt(el, Number(el.value));
    } else if (el.type === "color") {
      el.value = (v || "#000000").slice(0, 7).toLowerCase();
    } else if (el.classList.contains("hotkey")) {
      if (!el.classList.contains("capturing")) el.value = v ? hkLabel(v) : T("Aucun");
    }
  });
  document.querySelectorAll("[data-hk]").forEach((k) => {
    const v = cfg.hotkeys[k.dataset.hk];
    k.textContent = v ? hkLabel(v) : T("aucun");
  });
  document.querySelectorAll(".swatches").forEach((box) => {
    const cur = (getPath(cfg, box.dataset.for) || "").toUpperCase();
    box.querySelectorAll("button").forEach((b) => b.classList.toggle("on", b.dataset.color === cur));
  });
  document.getElementById("master-label").textContent = T(cfg.enabled ? "Effets activés" : "Effets en pause");
  // Réglages grisés quand l'effet est désactivé.
  dim("#ring .card", cfg.ring.enabled);
  dim("#clicks .card", cfg.clicks.enabled);
  dim("#trail .card:first-of-type", cfg.trail.enabled);
  dimRow("clicks.sound_volume", cfg.clicks.sound);
  dimRow("ring.pulse_period_ms", cfg.ring.pulse);
  dim("#keys .card", cfg.keys.enabled);
  document.getElementById("new-session").disabled = !cfg.session.enabled;
  const shape = cfg.spotlight.shape;
  document.getElementById("spot-on").checked = cfg.spotlight.mode !== "Off";
  dimRow("spotlight.radius", shape === "Circle");
  dimRow("spotlight.band_size", shape === "HBand" || shape === "VBand");
  renderProfiles();
  renderFormation();
  checkHotkeys();
}

function dim(sel, on) {
  const rows = document.querySelectorAll(sel + " .row");
  rows.forEach((r, i) => { if (i > 0) r.classList.toggle("disabled", !on); });
}
function dimRow(path, on) {
  const el = document.querySelector(`[data-path="${path}"]`);
  if (el) el.closest(".row").classList.toggle("disabled", !on);
}

// ---------- Liaison des contrôles ----------
function bind() {
  document.querySelectorAll("[data-path]").forEach((el) => {
    const path = el.dataset.path;
    if (el.classList.contains("segmented")) {
      el.addEventListener("click", (e) => {
        const b = e.target.closest("button");
        if (!b) return;
        const raw = b.dataset.value;
        setPath(cfg, path, el.hasAttribute("data-number") ? Number(raw) : el.hasAttribute("data-bool") ? raw === "true" : raw);
        // Changer de forme pendant que le spotlight est allumé : la nouvelle forme s'affiche tout de suite.
        if (path === "spotlight.shape" && cfg.spotlight.mode !== "Off") cfg.spotlight.mode = b.dataset.value;
        render();
        // Changement de langue : la fenêtre se recharge pour se traduire.
        if (path === "language") api.set(cfg, true).then(() => location.reload()).catch(showError);
        else pushSave();
      });
    } else if (el.type === "checkbox") {
      el.addEventListener("change", () => { setPath(cfg, path, el.checked); render(); pushSave(); });
    } else if (el.type === "range") {
      el.addEventListener("input", () => {
        dragging = true;
        setPath(cfg, path, Number(el.value));
        el.nextElementSibling.textContent = fmt(el, Number(el.value));
        pushLive();
      });
      el.addEventListener("change", () => { dragging = false; pushSave(); });
    } else if (el.type === "color") {
      el.addEventListener("input", () => { setPath(cfg, path, el.value.toUpperCase()); render(); pushLive(); });
      el.addEventListener("change", () => pushSave());
    } else if (el.classList.contains("hotkey")) {
      bindHotkey(el, path);
    }
  });

  document.querySelectorAll(".swatches").forEach((box) => {
    RING_PRESETS.forEach((c) => {
      const b = document.createElement("button");
      b.dataset.color = c;
      b.style.background = c;
      b.title = c;
      b.addEventListener("click", () => { setPath(cfg, box.dataset.for, c); render(); pushSave(); });
      box.appendChild(b);
    });
  });

  document.getElementById("spot-on").addEventListener("change", (e) => {
    cfg.spotlight.mode = e.target.checked ? cfg.spotlight.shape : "Off";
    render();
    pushSave();
  });
  document.getElementById("open-dir").addEventListener("click", () => api.openDir());
  document.getElementById("new-session").addEventListener("click", async () => {
    await api.action("new_session");
    toast(T("Nouvelle session : les prochains éléments iront dans un nouveau dossier"));
    setTimeout(showSession, 300);
  });
  document.getElementById("open-session").addEventListener("click", () => api.action("open_session"));
  const formation = document.getElementById("formation");
  const setFormation = async () => {
    cfg = await api.setFormation(formation.value);
    render();
    showSession();
    toast(cfg.session.formation ? T("Formation : {}", cfg.session.formation) : T("Aucune formation"));
  };
  document.getElementById("formation-set").addEventListener("click", setFormation);
  formation.addEventListener("keydown", (e) => { if (e.key === "Enter") setFormation(); });
  document.getElementById("profile-add").addEventListener("click", addProfile);
  document.getElementById("profile-name").addEventListener("keydown", (e) => { if (e.key === "Enter") addProfile(); });
  document.getElementById("reset").addEventListener("click", async () => {
    if (!confirm(T("Remettre tous les réglages et raccourcis par défaut ? (Les profils sont conservés.)"))) return;
    const keep = { profiles: cfg.profiles, active_profile: "", language: cfg.language, session: cfg.session };
    cfg = Object.assign(await api.defaults(), keep);
    render();
    pushSave();
    toast(T("Réglages d'origine restaurés"));
  });

  // Navigation entre les pages.
  const go = () => {
    const id = (location.hash || "#general").slice(1);
    document.querySelectorAll(".page").forEach((p) => p.classList.toggle("active", p.id === id));
    document.querySelectorAll("nav a").forEach((a) => a.classList.toggle("active", a.getAttribute("href") === "#" + id));
  };
  window.addEventListener("hashchange", go);
  go();
}

// ---------- Profils ----------
const BUILTIN = ["Tuto", "Classe virtuelle", "Stream", "Démo logiciel"];
const profileName = (n) => (BUILTIN.includes(n) ? T(n) : n);

function renderProfiles() {
  const list = document.getElementById("profile-list");
  list.replaceChildren();
  cfg.profiles.forEach((p, i) => {
    const row = document.createElement("div");
    row.className = "row";
    const text = document.createElement("div");
    text.className = "row-text";
    const name = document.createElement("strong");
    name.textContent = profileName(p.name);
    if (p.name === cfg.active_profile) {
      const b = document.createElement("span");
      b.className = "badge";
      b.textContent = T("actif");
      name.appendChild(b);
    }
    const desc = document.createElement("span");
    desc.textContent = describe(p.settings);
    text.append(name, desc);
    const btns = document.createElement("div");
    btns.className = "btns";
    const button = (label, title, fn, cls = "") => {
      const b = document.createElement("button");
      b.className = "btn small " + cls;
      b.textContent = T(label);
      if (title) b.title = T(title);
      b.addEventListener("click", fn);
      btns.appendChild(b);
    };
    button("Appliquer", "", () => applyProfile(i));
    button("Mettre à jour", "Remplace les réglages de ce profil par les réglages actuels", () => {
      cfg.profiles[i].settings = snapshot(cfg);
      cfg.active_profile = p.name;
      pushSave();
      toast(T("Profil « {} » mis à jour", profileName(p.name)));
    });
    button("Renommer", "", () => {
      const n = prompt(T("Nouveau nom du profil"), profileName(p.name));
      if (!n || !n.trim()) return;
      if (cfg.active_profile === p.name) cfg.active_profile = n.trim();
      cfg.profiles[i].name = n.trim();
      pushSave();
    });
    button("Supprimer", "", () => {
      if (!confirm(T("Supprimer le profil « {} » ?", profileName(p.name)))) return;
      cfg.profiles.splice(i, 1);
      pushSave();
    }, "danger");
    row.append(text, btns);
    list.appendChild(row);
  });
}

/// Résumé lisible d'un profil : effets allumés.
function describe(s) {
  const on = [];
  if (!s.enabled) return T("Effets en pause");
  if (s.spotlight.mode !== "Off") on.push(T("spotlight"));
  if (s.ring.enabled) on.push(T("anneau"));
  if (s.clicks.enabled) on.push(T(s.clicks.sound ? "clics avec son" : "clics"));
  if (s.trail.enabled) on.push(T("traînée"));
  if (s.keys.enabled) on.push(T("touches"));
  on.push(s.record.fps + " " + T("i/s"));
  return on.join(" · ");
}

function applyProfile(i) {
  const p = cfg.profiles[i];
  const folder = cfg.record.folder;
  Object.assign(cfg, structuredClone(p.settings));
  cfg.record.folder = folder;
  cfg.active_profile = p.name;
  render();
  pushSave();
  toast(T("Profil : {}", profileName(p.name)));
}

function addProfile() {
  const input = document.getElementById("profile-name");
  const name = input.value.trim();
  if (!name) { input.focus(); return; }
  if (cfg.profiles.some((p) => p.name.toLowerCase() === name.toLowerCase())) { toast(T("Ce nom existe déjà")); return; }
  cfg.profiles.push({ name, settings: snapshot(cfg) });
  cfg.active_profile = name;
  input.value = "";
  pushSave();
  toast(T("Profil « {} » créé", name));
}

function renderFormation() {
  const input = document.getElementById("formation");
  if (document.activeElement !== input) input.value = cfg.session.formation || "";
  const dl = document.getElementById("formation-list");
  dl.replaceChildren(...cfg.session.formations.map((f) => Object.assign(document.createElement("option"), { value: f })));
  document.querySelectorAll("#formation, #formation-set").forEach((e) => (e.disabled = !cfg.session.enabled));
}

// ---------- Raccourcis ----------
function keyName(code) {
  if (code >= 65 && code <= 90) return String.fromCharCode(code);
  if (code >= 48 && code <= 57) return String.fromCharCode(code);
  if (code >= 96 && code <= 105) return null; // pavé numérique : non géré pour l'instant
  if (code >= 112 && code <= 135) return "F" + (code - 111);
  return { 32: "Espace", 13: "Entrée", 9: "Tab", 19: "Pause", 45: "Inser", 46: "Suppr", 36: "Début", 35: "Fin" }[code] || null;
}

function bindHotkey(el, path) {
  el.addEventListener("focus", () => { el.classList.add("capturing"); el.value = T("Appuie sur les touches…"); });
  el.addEventListener("blur", () => { el.classList.remove("capturing"); render(); });
  el.addEventListener("keydown", (e) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.keyCode === 27) { el.blur(); return; }
    if (e.keyCode === 8 || (e.keyCode === 46 && !e.ctrlKey && !e.altKey && !e.shiftKey)) {
      setPath(cfg, path, null);
      el.blur();
      pushSave();
      return;
    }
    if ([16, 17, 18, 91, 92].includes(e.keyCode)) {
      el.value = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && T("Maj"), e.metaKey && "Win"].filter(Boolean).join(" + ") + " + …";
      return;
    }
    const key = keyName(e.keyCode);
    if (!key) { toast(T("Cette touche n'est pas prise en charge")); return; }
    const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Maj", e.metaKey && "Win"].filter(Boolean);
    if (mods.length === 0 && !/^F\d+$/.test(key)) { toast(T("Ajoute au moins Ctrl, Alt ou Maj")); return; }
    setPath(cfg, path, [...mods, key].join("+"));
    el.blur();
    pushSave();
  });
}

// Raccourcis déjà pris par Teams, Office ou Windows (liste non exhaustive).
const TAKEN = {
  "Ctrl+Maj+M": "Teams (micro)", "Ctrl+Maj+O": "Teams (caméra)", "Ctrl+Maj+E": "Teams (partage d'écran)",
  "Ctrl+Maj+K": "Teams (lever la main)", "Ctrl+Maj+A": "Teams (accepter un appel)", "Ctrl+Maj+H": "Teams (raccrocher)",
  "Ctrl+Maj+C": "Office (copier la mise en forme)", "Ctrl+Maj+V": "Office (coller la mise en forme)",
  "Ctrl+Maj+F": "Office (police)", "Ctrl+Maj+S": "Office (styles)", "Ctrl+Maj+N": "Windows (nouveau dossier)",
  "Ctrl+Maj+Échap": "Windows (gestionnaire des tâches)", "Alt+Tab": "Windows", "Alt+F4": "Windows (fermer)",
  "Ctrl+Alt+Suppr": "Windows", "Win+L": "Windows (verrouiller)", "Win+D": "Windows (bureau)",
  "Ctrl+Alt+E": "AltGr + E = € en AZERTY", "Ctrl+Alt+0": "AltGr + 0 = @ en AZERTY", "Ctrl+Alt+3": "AltGr + 3 = # en AZERTY",
  "Ctrl+Maj+L": "Office (liste à puces)", "Ctrl+Maj+D": "Word (double soulignement)", "Ctrl+Maj+T": "Navigateur (rouvrir un onglet)",
  "Ctrl+Alt+4": "AltGr + 4 = { en AZERTY", "Ctrl+Alt+5": "AltGr + 5 = [ en AZERTY", "Ctrl+Alt+8": "AltGr + 8 = \\ en AZERTY",
};

function checkHotkeys() {
  const warn = document.getElementById("hk-warn");
  const msgs = [];
  const seen = {};
  document.querySelectorAll(".hotkey").forEach((el) => {
    const v = getPath(cfg, el.dataset.path);
    let bad = false;
    if (v) {
      if (seen[v]) { msgs.push(T("{} est utilisé deux fois.", hkLabel(v))); bad = true; }
      seen[v] = true;
      if (TAKEN[v]) { msgs.push(T("{} est déjà utilisé par {}.", hkLabel(v), T(TAKEN[v]))); bad = true; }
      else if (/^Ctrl\+Alt\+[A-Z0-9]$/.test(v)) { msgs.push(T("{} correspond à AltGr sur un clavier AZERTY : ajoute Maj.", hkLabel(v))); bad = true; }
    }
    el.classList.toggle("conflict", bad);
  });
  warn.hidden = msgs.length === 0;
  warn.textContent = msgs.join(" ");
}

// ---------- Divers ----------
async function showSession() {
  const [current, root] = await api.session();
  const el = document.getElementById("session-dir");
  el.classList.toggle("mono", !!current);
  el.textContent = current
    || T("Pas encore de dossier : il sera créé dans {} au premier élément enregistré.", root);
}
let toastTimer;
function toast(msg) {
  const t = document.getElementById("toast");
  t.textContent = msg;
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), 2200);
}
function showError(e) { toast(String(e)); }

(async function main() {
  LANG = await api.lang();
  translatePage();
  bind();
  cfg = await api.get();
  render();
  document.getElementById("data-dir").textContent = await api.dataDir();
  showSession();
  window.addEventListener("focus", showSession);
  api.onChange((c) => { if (!dragging) { cfg = c; render(); } });
})();
