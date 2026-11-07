use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Raccourci clavier : modificateurs + une touche (code virtuel Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u32,
}

impl Hotkey {
    /// Analyse une chaîne du type `Ctrl+Alt+Maj+A`, `Ctrl + Maj + F9`, `Win+1`.
    pub fn parse(s: &str) -> Option<Self> {
        let mut hk = Hotkey { ctrl: false, alt: false, shift: false, win: false, vk: 0 };
        for part in s.split('+').map(|p| p.trim()).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "contrôle" => hk.ctrl = true,
                "alt" => hk.alt = true,
                "maj" | "shift" => hk.shift = true,
                "win" | "windows" => hk.win = true,
                key => {
                    if hk.vk != 0 {
                        return None;
                    }
                    hk.vk = key_to_vk(key)?;
                }
            }
        }
        (hk.vk != 0).then_some(hk)
    }
}

fn key_to_vk(key: &str) -> Option<u32> {
    let up = key.to_ascii_uppercase();
    let b = up.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit()) {
        return Some(b[0] as u32);
    }
    if let Some(n) = up.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x70 + n - 1);
        }
    }
    Some(match up.as_str() {
        "ECHAP" | "ÉCHAP" | "ESC" | "ESCAPE" => 0x1B,
        "ESPACE" | "SPACE" => 0x20,
        "ENTREE" | "ENTRÉE" | "ENTER" => 0x0D,
        "TAB" => 0x09,
        "PAUSE" => 0x13,
        "INSER" | "INSERT" => 0x2D,
        "SUPPR" | "DELETE" => 0x2E,
        "DEBUT" | "DÉBUT" | "HOME" => 0x24,
        "FIN" | "END" => 0x23,
        _ => return None,
    })
}

fn vk_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x1B => "Échap".into(),
        0x20 => "Espace".into(),
        0x0D => "Entrée".into(),
        0x09 => "Tab".into(),
        0x13 => "Pause".into(),
        0x2D => "Inser".into(),
        0x2E => "Suppr".into(),
        0x24 => "Début".into(),
        0x23 => "Fin".into(),
        _ => format!("VK{vk:02X}"),
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl { parts.push("Ctrl".into()); }
        if self.alt { parts.push("Alt".into()); }
        if self.shift { parts.push("Maj".into()); }
        if self.win { parts.push("Win".into()); }
        parts.push(vk_name(self.vk));
        write!(f, "{}", parts.join("+"))
    }
}

impl Serialize for Hotkey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Hotkey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Hotkey::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("raccourci invalide : {s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_fr() {
        let h = Hotkey::parse("Ctrl + Alt + Maj + S").unwrap();
        assert!(h.ctrl && h.alt && h.shift && !h.win);
        assert_eq!(h.vk, 'S' as u32);
        assert_eq!(h.to_string(), "Ctrl+Alt+Maj+S");
        assert_eq!(Hotkey::parse("Ctrl+F9").unwrap().vk, 0x78);
        assert!(Hotkey::parse("Ctrl+Alt").is_none());
        assert!(Hotkey::parse("A+B").is_none());
    }
}
