use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Couleur RGBA non prémultipliée, composantes 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: a as f32 / 255.0 }
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Color { a: a.clamp(0.0, 1.0), ..self }
    }

    /// Accepte `#RRGGBB` ou `#RRGGBBAA`.
    pub fn parse_hex(s: &str) -> Option<Self> {
        let h = s.trim().strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            6 => Some(Self::rgba8(byte(0)?, byte(2)?, byte(4)?, 255)),
            8 => Some(Self::rgba8(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if c(self.a) == 255 {
            format!("#{:02X}{:02X}{:02X}", c(self.r), c(self.g), c(self.b))
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", c(self.r), c(self.g), c(self.b), c(self.a))
        }
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("couleur invalide : {s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        assert_eq!(Color::parse_hex("#FF8000").unwrap().to_hex(), "#FF8000");
        assert_eq!(Color::parse_hex("#FF800080").unwrap().to_hex(), "#FF800080");
        assert!(Color::parse_hex("FF8000").is_none());
    }
}
