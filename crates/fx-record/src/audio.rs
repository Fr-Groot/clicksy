//! Son : capture WASAPI (crate MIT `wasapi`) du son du PC (« loopback ») ou du micro,
//! écrite en WAV 48 kHz stéréo flottant.
//!
//! Alignement : chaque paquet WASAPI porte son horodatage (horloge 100 ns commune). On écrit
//! du silence pour combler l'écart avec le début de l'enregistrement et les trous (le son du PC
//! ne produit aucun paquet tant que rien ne joue), si bien que l'échantillon n° k correspond
//! toujours à l'instant t0 + k / 48000.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

pub const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const BYTES_PER_FRAME: usize = 4 * CHANNELS as usize;

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    System,
    Mic,
}

pub fn record(kind: Kind, path: &Path, t0: i64, stop: &AtomicBool) -> Result<(), String> {
    let _ = wasapi::initialize_mta();
    let err = |e: wasapi::WasapiError| e.to_string();
    let enumerator = DeviceEnumerator::new().map_err(err)?;
    let device = match kind {
        // Loopback : on « écoute » la sortie audio par défaut.
        Kind::System => enumerator.get_default_device(Direction::Render),
        Kind::Mic => enumerator.get_default_device(Direction::Capture),
    }
    .map_err(|e| format!("aucun périphérique audio ({e})"))?;
    let mut client = device.get_iaudioclient().map_err(err)?;
    let format = WaveFormat::new(32, 32, SampleType::Float, RATE as usize, CHANNELS as usize, None);
    let (default_period, _) = client.get_device_period().map_err(err)?;
    let mode = StreamMode::PollingShared { autoconvert: true, buffer_duration_hns: default_period * 10 };
    client.initialize_client(&format, Direction::Capture, mode, None).map_err(err)?;
    let capture = client.get_audiocaptureclient().map_err(err)?;

    let mut wav = WavWriter::create(path).map_err(|e| e.to_string())?;
    let mut queue: VecDeque<u8> = VecDeque::new();
    client.start_stream().map_err(err)?;

    while !stop.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(10));
        loop {
            match capture.get_next_packet_size() {
                Ok(Some(n)) if n > 0 => {}
                Ok(_) => break,
                Err(e) => return Err(e.to_string()),
            }
            let before = queue.len();
            let info = capture.read_from_device_to_deque(&mut queue).map_err(err)?;
            let frames = (queue.len() - before) / BYTES_PER_FRAME;
            // Position attendue de ce paquet dans le fichier.
            let expected = ((info.timestamp as i64 - t0).max(0) as i128 * RATE as i128 / 10_000_000) as u64;
            if expected > wav.frames + RATE as u64 / 100 {
                wav.write_silence(expected - wav.frames).map_err(|e| e.to_string())?;
            }
            if info.flags.silent {
                queue.truncate(before);
                wav.write_silence(frames as u64).map_err(|e| e.to_string())?;
            } else {
                let (a, b) = queue.as_slices();
                wav.write_bytes(a).map_err(|e| e.to_string())?;
                wav.write_bytes(b).map_err(|e| e.to_string())?;
            }
            queue.clear();
        }
    }
    let _ = client.stop_stream();
    wav.finish().map_err(|e| e.to_string())
}

/// WAV flottant écrit au fil de l'eau ; les tailles de l'en-tête sont corrigées à la fin.
struct WavWriter {
    out: BufWriter<File>,
    frames: u64,
    pending: usize,
}

impl WavWriter {
    fn create(path: &Path) -> std::io::Result<Self> {
        let mut out = BufWriter::new(File::create(path)?);
        let byte_rate = RATE * BYTES_PER_FRAME as u32;
        out.write_all(b"RIFF")?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(b"WAVEfmt ")?;
        out.write_all(&16u32.to_le_bytes())?;
        out.write_all(&3u16.to_le_bytes())?; // IEEE float
        out.write_all(&CHANNELS.to_le_bytes())?;
        out.write_all(&RATE.to_le_bytes())?;
        out.write_all(&byte_rate.to_le_bytes())?;
        out.write_all(&(BYTES_PER_FRAME as u16).to_le_bytes())?;
        out.write_all(&32u16.to_le_bytes())?;
        out.write_all(b"data")?;
        out.write_all(&0u32.to_le_bytes())?;
        Ok(WavWriter { out, frames: 0, pending: 0 })
    }

    fn write_bytes(&mut self, b: &[u8]) -> std::io::Result<()> {
        self.out.write_all(b)?;
        self.pending += b.len();
        self.frames += (self.pending / BYTES_PER_FRAME) as u64;
        self.pending %= BYTES_PER_FRAME;
        Ok(())
    }

    fn write_silence(&mut self, frames: u64) -> std::io::Result<()> {
        let zeros = [0u8; 4096];
        let mut left = frames as usize * BYTES_PER_FRAME;
        while left > 0 {
            let n = left.min(zeros.len());
            self.out.write_all(&zeros[..n])?;
            left -= n;
        }
        self.frames += frames;
        Ok(())
    }

    fn finish(mut self) -> std::io::Result<()> {
        let data = self.frames * BYTES_PER_FRAME as u64;
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| e.into_error())?;
        f.seek(SeekFrom::Start(4))?;
        f.write_all(&((36 + data).min(u32::MAX as u64) as u32).to_le_bytes())?;
        f.seek(SeekFrom::Start(40))?;
        f.write_all(&(data.min(u32::MAX as u64) as u32).to_le_bytes())?;
        Ok(())
    }
}
