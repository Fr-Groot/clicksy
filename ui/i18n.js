"use strict";
// Traduction de la fenêtre de réglages. Le français est la langue source :
// chaque texte français (HTML normalisé) a son équivalent anglais dans EN.
let LANG = "fr";

/** Traduit un texte et remplace chaque {} par l'argument suivant. */
function T(fr, ...args) {
  let s = LANG === "en" && EN[fr] !== undefined ? EN[fr] : fr;
  for (const a of args) s = s.replace("{}", a);
  return s;
}

/** « Ctrl+Alt+Maj+K » → « Ctrl+Alt+Shift+K » en anglais. */
function hkLabel(v) {
  if (LANG !== "en") return v;
  const map = { "Maj": "Shift", "Échap": "Esc", "Espace": "Space", "Entrée": "Enter", "Inser": "Insert", "Suppr": "Delete", "Début": "Home", "Fin": "End" };
  return v.split("+").map((p) => map[p] || p).join("+");
}

const norm = (html) => html.replace(/\s+/g, " ").trim();

/** Remplace les textes de la page par leur traduction (avant le premier affichage). */
function translatePage() {
  document.documentElement.lang = LANG;
  if (LANG !== "en") return;
  document.title = T(document.title);
  // Conteneurs d'abord (leur traduction contient déjà les <kbd> traduits), puis les éléments isolés.
  const groups = [
    "h1, h2, p, .row-text > strong, .row-text > span, nav a, .label, .keys > div",
    "button, label > span, kbd:not([data-hk]), strong",
  ];
  for (const sel of groups) {
    document.querySelectorAll(sel).forEach((el) => {
      const key = norm(el.innerHTML);
      if (EN[key] !== undefined) el.innerHTML = EN[key];
    });
  }
  document.querySelectorAll("[placeholder]").forEach((el) => (el.placeholder = T(el.placeholder)));
  document.querySelectorAll("[title]").forEach((el) => (el.title = T(el.title)));
}

