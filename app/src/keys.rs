//! Affichage des touches et combinaisons pressées (« Ctrl + C », « Maj + Clic gauche »).
//!
//! Rien n'est écrit sur le disque : les libellés vivent quelques secondes en mémoire, le temps
//! de l'affichage. Par défaut, seuls les raccourcis s'affichent (pas le texte tapé). En mode
//! « toutes les touches », la saisie est masquée dès que le champ actif est un mot de passe.

use fx_bus::{EventKind, MouseButton};
use fx_core::{Config, Hotkey, KeyShow, Shared};
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyNameTextW, MapVirtualKeyW, MAPVK_VK_TO_CHAR};

/// Au-delà, les plus anciennes touches d'une suite tapée disparaissent.
const MAX_SEQ: usize = 14;
/// Texte affiché à la place de la saisie dans un champ mot de passe.
const MASKED: &str = "Saisie masquée";

#[derive(Default)]
pub struct Keys {
    lctrl: bool,
    rctrl: bool,
    alt: bool,
    /// Alt de droite : AltGr sur les claviers français (Windows y ajoute un faux Ctrl gauche).
    altgr: bool,
    shift: bool,
    win: bool,
    /// Touches enfoncées (pour ignorer la répétition automatique).
    down: HashSet<u32>,
    /// Une autre touche ou un clic a eu lieu pendant que Win était enfoncée.
    win_used: bool,
    uia: Option<IUIAutomation>,
    uia_failed: bool,
    /// Dernière réponse « le champ actif est-il un mot de passe ? ».
    pw_cache: Option<(Instant, bool)>,
}

impl Keys {
    pub fn on_event(&mut self, shared: &Shared, kind: &EventKind) {
        match *kind {
            EventKind::KeyDown { vk, injected, .. } => self.key(shared, vk, injected, true),
            EventKind::KeyUp { vk, injected, .. } => self.key(shared, vk, injected, false),
            EventKind::MouseDown { button, x, y } => {
                // Le champ actif peut changer après un clic.
                self.pw_cache = None;
                if self.win {
                    self.win_used = true;
                }
                let cfg = shared.config.load();
                if !self.visible(shared, &cfg) || !cfg.keys.mouse || shared.modes.on_toolbar(x, y) {
                    return;
                }
                if !(self.ctrl() || self.alt || self.shift || self.win) {
                    return;
                }
                let name = match button {
                    MouseButton::Left => "Clic gauche",
                    MouseButton::Right => "Clic droit",
                    MouseButton::Middle => "Clic molette",
                    MouseButton::X1 => "Bouton précédent",
                    MouseButton::X2 => "Bouton suivant",
                };
                let mut parts = self.mods();
                parts.push(name.into());
                show_combo(shared, parts, cfg.keys.duration_ms);
            }
            EventKind::Wheel { delta, horizontal: false, x, y } => {
                let cfg = shared.config.load();
                if !self.visible(shared, &cfg) || !cfg.keys.mouse || shared.modes.on_toolbar(x, y) {
                    return;
                }
                // La molette seule fait défiler : rien à montrer. Avec Ctrl (zoom), Maj… oui.
                if !(self.ctrl() || self.alt || self.shift) {
                    return;
                }
                let mut parts = self.mods();
                parts.push(if delta > 0 { "Molette ↑" } else { "Molette ↓" }.into());
                show_combo(shared, parts, cfg.keys.duration_ms);
            }
            _ => {}
        }
    }

    fn ctrl(&self) -> bool {
        (self.lctrl || self.rctrl) && !self.altgr
    }

    fn visible(&self, shared: &Shared, cfg: &Config) -> bool {
        cfg.keys.enabled && shared.modes.select.load(Ordering::Acquire) == 0
    }

    /// Modificateurs enfoncés, dans l'ordre habituel.
    fn mods(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.ctrl() {
            v.push("Ctrl".into());
        }
        if self.alt {
            v.push("Alt".into());
        }
        if self.altgr {
            v.push("AltGr".into());
        }
        if self.shift {
            v.push("Maj".into());
        }
        if self.win {
            v.push("Win".into());
        }
        v
    }

    fn key(&mut self, shared: &Shared, vk: u32, injected: bool, down: bool) {
        // Modificateurs : on suit leur état, ils ne s'affichent qu'avec une autre touche.
        let modifier = match vk {
            0xA0 | 0xA1 | 0x10 => Some(&mut self.shift),
            0xA2 | 0x11 => Some(&mut self.lctrl),
            0xA3 => Some(&mut self.rctrl),
            0xA4 | 0x12 => Some(&mut self.alt),
            0xA5 => Some(&mut self.altgr),
            0x5B | 0x5C => Some(&mut self.win),
            _ => None,
        };
        if let Some(m) = modifier {
            let was = *m;
            *m = down;
            if matches!(vk, 0x5B | 0x5C) {
                if down && !was {
                    self.win_used = false;
                } else if !down && was && !self.win_used {
                    // Win seule : ouvre le menu Démarrer.
                    let cfg = shared.config.load();
                    if self.visible(shared, &cfg) && !injected {
                        show_combo(shared, vec!["Win".into()], cfg.keys.duration_ms);
                    }
                }
            }
            return;
        }

        if !down {
            self.down.remove(&vk);
            return;
        }
        // Répétition automatique d'une touche maintenue : on ne l'affiche qu'une fois.
        if !self.down.insert(vk) {
            return;
        }
        if self.win {
            self.win_used = true;
        }
        if matches!(vk, 0x09 | 0x0D) {
            // Tab ou Entrée : le champ actif change souvent.
            self.pw_cache = None;
        }
        let cfg = shared.config.load();
        if !self.visible(shared, &cfg) {
            return;
        }
        // Les raccourcis de Clicksy eux-mêmes ne s'affichent pas.
        let pressed = Hotkey {
            ctrl: self.lctrl || self.rctrl,
            alt: self.alt || self.altgr,
            shift: self.shift,
            win: self.win,
            vk,
        };
        if cfg.hotkeys.table().iter().any(|(h, _)| *h == Some(pressed)) {
            return;
        }

        let name = key_name(vk);
        let with_mod = self.ctrl() || self.alt || self.win;
        let combo = with_mod || alone_shown(vk) || (self.shift && !is_char(vk));
        let dur = cfg.keys.duration_ms;
        if combo {
            let mut parts = self.mods();
            parts.push(name);
            show_combo(shared, parts, dur);
        } else if cfg.keys.all_keys {
            if self.password_focus() {
                show_masked(shared);
            } else {
                show_typed(shared, name, dur);
            }
        }
    }

    /// Le champ qui a le focus est-il un mot de passe ? (UI Automation, réponse gardée 400 ms.)
    /// En cas de doute (accès refusé, appli figée…), on répond oui : mieux vaut masquer.
    fn password_focus(&mut self) -> bool {
        if let Some((t, v)) = self.pw_cache {
            if t.elapsed() < Duration::from_millis(400) {
                return v;
            }
        }
        let v = unsafe { self.query_password() }.unwrap_or(true);
        self.pw_cache = Some((Instant::now(), v));
        v
    }

    unsafe fn query_password(&mut self) -> Option<bool> {
        if self.uia.is_none() && !self.uia_failed {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            match CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                Ok(u) => self.uia = Some(u),
                Err(_) => self.uia_failed = true,
            }
        }
        let el = self.uia.as_ref()?.GetFocusedElement().ok()?;
        Some(el.CurrentIsPassword().ok()?.as_bool())
    }
}

/// Affiche une combinaison ; la même combinaison répétée affiche « ×2 », « ×3 »…
fn show_combo(shared: &Shared, parts: Vec<String>, dur_ms: u32) {
    let now = Instant::now();
    let dur = Duration::from_millis(dur_ms as u64);
    {
        let mut fx = shared.fx.lock().unwrap();
        match fx.keys.as_mut() {
            Some(k) if k.combo && k.parts == parts && now.saturating_duration_since(k.start) < dur => {
                k.count += 1;
                k.start = now;
            }
            _ => fx.keys = Some(KeyShow { parts, combo: true, count: 1, start: now }),
        }
    }
    shared.wake();
}

/// Mode « toutes les touches » : les touches tapées à la suite s'ajoutent sur une même ligne.
fn show_typed(shared: &Shared, name: String, dur_ms: u32) {
    let now = Instant::now();
    let dur = Duration::from_millis(dur_ms as u64);
    {
        let mut fx = shared.fx.lock().unwrap();
        match fx.keys.as_mut() {
            Some(k) if !k.combo && now.saturating_duration_since(k.start) < dur => {
                k.parts.push(name);
                let excess = k.parts.len().saturating_sub(MAX_SEQ);
                k.parts.drain(..excess);
                k.start = now;
            }
            _ => fx.keys = Some(KeyShow { parts: vec![name], combo: false, count: 1, start: now }),
        }
    }
    shared.wake();
}

/// Champ mot de passe : une seule étiquette, sans compteur (qui trahirait la longueur).
fn show_masked(shared: &Shared) {
    shared.fx.lock().unwrap().keys = Some(KeyShow { parts: vec![MASKED.into()], combo: true, count: 1, start: Instant::now() });
    shared.wake();
}

/// Touches qui produisent du texte (lettres, chiffres, ponctuation, espace, pavé numérique).
fn is_char(vk: u32) -> bool {
    matches!(vk, 0x20 | 0x30..=0x39 | 0x41..=0x5A | 0x60..=0x6F | 0xBA..=0xC0 | 0xDB..=0xDF | 0xE2)
}

/// Touches affichées même sans modificateur : F1 à F24, Échap, Impr. écran.
fn alone_shown(vk: u32) -> bool {
    matches!(vk, 0x70..=0x87 | 0x1B | 0x2C)
}

fn key_name(vk: u32) -> String {
    let fixed = match vk {
        0x08 => "Retour arrière",
        0x09 => "Tab",
        0x0D => "Entrée",
        0x13 => "Pause",
        0x14 => "Verr. Maj",
        0x1B => "Échap",
        0x20 => "Espace",
        0x21 => "Page préc.",
        0x22 => "Page suiv.",
        0x23 => "Fin",
        0x24 => "Début",
        0x25 => "←",
        0x26 => "↑",
        0x27 => "→",
        0x28 => "↓",
        0x2C => "Impr. écran",
        0x2D => "Inser",
        0x2E => "Suppr",
        0x5D => "Menu",
        0x6A => "*",
        0x6B => "+",
        0x6D => "-",
        0x6E => ".",
        0x6F => "/",
        0x90 => "Verr. Num",
        0x91 => "Arrêt défil",
        0xAD => "Muet",
        0xAE => "Volume -",
        0xAF => "Volume +",
        0xB0 => "Piste suivante",
        0xB1 => "Piste précédente",
        0xB3 => "Lecture/Pause",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.into();
    }
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(String::from).unwrap_or_default(),
        0x60..=0x69 => (vk - 0x60).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => unsafe {
            // Ponctuation : le caractère de la touche selon la disposition du clavier (AZERTY…).
            // Le bit de poids fort signale une touche morte (^, ¨) : on l'enlève.
            let c = MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) & 0x7FFF_FFFF;
            if let Some(ch) = char::from_u32(c).filter(|c| !c.is_control() && *c != '\0') {
                return ch.to_uppercase().collect();
            }
            let scan = windows::Win32::UI::Input::KeyboardAndMouse::MapVirtualKeyW(vk, windows::Win32::UI::Input::KeyboardAndMouse::MAPVK_VK_TO_VSC);
            let mut buf = [0u16; 64];
            let n = GetKeyNameTextW((scan << 16) as i32, &mut buf);
            if n > 0 {
                String::from_utf16_lossy(&buf[..n as usize])
            } else {
                format!("Touche {vk}")
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms_des_touches() {
        assert_eq!(key_name(0x43), "C");
        assert_eq!(key_name(0x74), "F5");
        assert_eq!(key_name(0x25), "←");
        assert_eq!(key_name(0x2E), "Suppr");
        assert!(is_char(0x41) && is_char(0x20) && !is_char(0x25));
        assert!(alone_shown(0x70) && !alone_shown(0x41));
    }
}
