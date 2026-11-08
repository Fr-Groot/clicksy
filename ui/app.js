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
  session: { enabled: true, folder: "" },
  hotkeys: { toggle_all: "Ctrl+Alt+Maj+A", cycle_spotlight: "Ctrl+Alt+Maj+S", toggle_ring: "Ctrl+Alt+Maj+R", toggle_clicks: "Ctrl+Alt+Maj+C",
             toggle_draw: "Ctrl+Alt+Maj+D", toggle_laser: "Ctrl+Alt+Maj+L", toggle_trail: "Ctrl+Alt+Maj+T",
             toggle_toolbar: "Ctrl+Alt+Maj+O", cycle_board: "Ctrl+Alt+Maj+W", screenshot: "Ctrl+Alt+Maj+P",
             toggle_record: "Ctrl+Alt+Maj+E", toggle_keys: "Ctrl+Alt+Maj+K" },
};
const api = TAURI
  ? {
      get: () => TAURI.core.invoke("get_config"),
      defaults: () => TAURI.core.invoke("default_config"),
      set: (config, save) => TAURI.core.invoke("set_config", { config, save }),
      openDir: () => TAURI.core.invoke("open_data_dir"),
      dataDir: () => TAURI.core.invoke("data_dir"),
      action: (name) => TAURI.core.invoke("run_action", { name }),
      session: () => TAURI.core.invoke("session_info"),
      onChange: (cb) => TAURI.event.listen("config-changed", (e) => cb(e.payload)),
    }
  : {
      get: async () => structuredClone(MOCK),
      defaults: async () => structuredClone(MOCK),
      set: async (c) => c,
      openDir: async () => {},
      dataDir: async () => "(aperçu hors application)",
      action: async () => {},
      session: async () => ["", "C:\Users\…\Documents\Clicksy"],
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
  if (unit === "ms") return (v >= 1000 ? (v / 1000).toFixed(1).replace(".", ",") + " s" : v + " ms");
  return Number(v).toLocaleString("fr-FR", { maximumFractionDigits: 1 }) + " px";
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
      if (!el.classList.contains("capturing")) el.value = v || "Aucun";
    }
  });
  document.querySelectorAll("[data-hk]").forEach((k) => {
    const v = cfg.hotkeys[k.dataset.hk];
    k.textContent = v || "aucun";
  });
  document.querySelectorAll(".swatches").forEach((box) => {
    const cur = (getPath(cfg, box.dataset.for) || "").toUpperCase();
    box.querySelectorAll("button").forEach((b) => b.classList.toggle("on", b.dataset.color === cur));
  });
  document.getElementById("master-label").textContent = cfg.enabled ? "Effets activés" : "Effets en pause";
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
        pushSave();
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
    toast("Nouvelle session : les prochains éléments iront dans un nouveau dossier");
    setTimeout(showSession, 300);
  });
  document.getElementById("open-session").addEventListener("click", () => api.action("open_session"));
  document.getElementById("reset").addEventListener("click", async () => {
    if (!confirm("Remettre tous les réglages et raccourcis par défaut ?")) return;
    cfg = await api.defaults();
    render();
    pushSave();
    toast("Réglages d'origine restaurés");
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

// ---------- Raccourcis ----------
function keyName(code) {
  if (code >= 65 && code <= 90) return String.fromCharCode(code);
  if (code >= 48 && code <= 57) return String.fromCharCode(code);
  if (code >= 96 && code <= 105) return null; // pavé numérique : non géré pour l'instant
  if (code >= 112 && code <= 135) return "F" + (code - 111);
  return { 32: "Espace", 13: "Entrée", 9: "Tab", 19: "Pause", 45: "Inser", 46: "Suppr", 36: "Début", 35: "Fin" }[code] || null;
}

function bindHotkey(el, path) {
  el.addEventListener("focus", () => { el.classList.add("capturing"); el.value = "Appuie sur les touches…"; });
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
      el.value = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Maj", e.metaKey && "Win"].filter(Boolean).join(" + ") + " + …";
      return;
    }
    const key = keyName(e.keyCode);
    if (!key) { toast("Cette touche n'est pas prise en charge"); return; }
    const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Maj", e.metaKey && "Win"].filter(Boolean);
    if (mods.length === 0 && !/^F\d+$/.test(key)) { toast("Ajoute au moins Ctrl, Alt ou Maj"); return; }
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
      if (seen[v]) { msgs.push(`${v} est utilisé deux fois.`); bad = true; }
      seen[v] = true;
      if (TAKEN[v]) { msgs.push(`${v} est déjà utilisé par ${TAKEN[v]}.`); bad = true; }
      else if (/^Ctrl\+Alt\+[A-Z0-9]$/.test(v)) { msgs.push(`${v} correspond à AltGr sur un clavier AZERTY : ajoute Maj.`); bad = true; }
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
    || `Pas encore de dossier : il sera créé dans ${root} au premier élément enregistré.`;
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
  bind();
  cfg = await api.get();
  render();
  document.getElementById("data-dir").textContent = await api.dataDir();
  showSession();
  window.addEventListener("focus", showSession);
  api.onChange((c) => { if (!dragging) { cfg = c; render(); } });
})();
