//! Son de clic : un WAV très court synthétisé en mémoire, joué en asynchrone.
//!
//! `PlaySoundW(SND_MEMORY | SND_ASYNC)` exige que le tampon reste valide pendant la
//! lecture : on garde donc un tampon par niveau de volume (11 niveaux, ~3 Ko chacun),
//! créés à la demande et jamais libérés.

use std::sync::{Mutex, OnceLock};
use windows::core::PCWSTR;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

const RATE: u32 = 44_100;
const LEVELS: usize = 11;

static BUFFERS: OnceLock<Mutex<[Option<&'static [u8]>; LEVELS]>> = OnceLock::new();

/// Joue le son de clic (interrompt le précédent). `volume` entre 0 et 1.
pub fn play_click(volume: f32) {
    let level = ((volume.clamp(0.0, 1.0) * (LEVELS - 1) as f32).round()) as usize;
    if level == 0 {
        return;
    }
    let buf = {
        let mut cache = BUFFERS.get_or_init(|| Mutex::new([None; LEVELS])).lock().unwrap();
        *cache[level].get_or_insert_with(|| Box::leak(synth_click(level as f32 / (LEVELS - 1) as f32).into_boxed_slice()))
    };
    unsafe {
        let _ = PlaySoundW(PCWSTR(buf.as_ptr() as *const u16), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}

/// Clic sec de 30 ms : attaque brève de bruit + sinus amorti.
fn synth_click(volume: f32) -> Vec<u8> {
    let n = (RATE as f32 * 0.030) as usize;
    let mut seed: u32 = 0x1234_5678;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = ((seed >> 9) as f32 / (1u32 << 23) as f32) * 2.0 - 1.0;
        let env_noise = (-t * 900.0).exp();
        let env_tone = (-t * 160.0).exp();
        let tone = (2.0 * std::f32::consts::PI * 1800.0 * t).sin();
        let v = (0.55 * noise * env_noise + 0.45 * tone * env_tone) * volume * 0.8;
        samples.push((v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
    }
    wav_mono16(&samples)
}

fn wav_mono16(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

#[cfg(test)]
mod tests {
    #[test]
    fn wav_header() {
        let w = super::synth_click(1.0);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(w.len(), 44 + 2 * (44_100.0f32 * 0.030) as usize);
    }
}
