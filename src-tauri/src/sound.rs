//! Plays alert sounds from the background: the built-in tones, synthesised
//! here so no audio files ship with the app, or a file you imported. The
//! audio device is opened only while a sound plays.

use std::io::Cursor;
use std::path::Path;

use anyhow::{Result, anyhow};
use rodio::{Decoder, OutputStream, Sink, Source, buffer::SamplesBuffer};

use crate::status_alerts::Sound;

const RATE: u32 = 44_100;

/// A waveform (phase 0–1 to sample) and its notes as [frequency Hz, start s, length s].
type Tone = (fn(f32) -> f32, &'static [[f32; 3]]);

/// Each built-in tone.
fn notes(tone: &str) -> Option<Tone> {
    let sine = |p: f32| (p * std::f32::consts::TAU).sin();
    let triangle = |p: f32| 4.0 * (p - (p + 0.5).floor()).abs() - 1.0;
    Some(match tone {
        "chime" => (sine, &[[880.0, 0.0, 0.5], [1320.0, 0.12, 0.6]]),
        "ping" => (sine, &[[1568.0, 0.0, 0.25]]),
        "knock" => (triangle, &[[220.0, 0.0, 0.09], [220.0, 0.16, 0.09]]),
        "rise" => (
            sine,
            &[[523.0, 0.0, 0.18], [659.0, 0.12, 0.18], [784.0, 0.24, 0.35]],
        ),
        "fall" => (triangle, &[[659.0, 0.0, 0.2], [440.0, 0.16, 0.4]]),
        "soft" => (sine, &[[392.0, 0.0, 0.7]]),
        _ => return None,
    })
}

/// The tone as samples: each note with a quick attack and an exponential
/// release, so it never clicks.
fn tone_samples(tone: &str) -> Option<Vec<f32>> {
    let (wave, notes) = notes(tone)?;
    let end = notes.iter().map(|n| n[1] + n[2]).fold(0.0, f32::max) + 0.05;
    let mut out = vec![0.0f32; (end * RATE as f32) as usize];
    for &[hz, at, len] in notes {
        let start = (at * RATE as f32) as usize;
        for i in 0..(len * RATE as f32) as usize {
            let t = i as f32 / RATE as f32;
            let attack = (t / 0.015).min(1.0);
            let release = (-6.0 * t / len).exp();
            if let Some(s) = out.get_mut(start + i) {
                *s += 0.35 * attack * release * wave((hz * t).fract());
            }
        }
    }
    Some(out)
}

/// Plays a sound at `volume` (0–1) and blocks until it ends; call it off the
/// main thread.
pub fn play(sound: &Sound, volume: f32, data_dir: &Path) -> Result<()> {
    let (_stream, handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&handle)?;
    sink.set_volume(volume.clamp(0.0, 1.0));
    match sound {
        Sound::Tone { tone } => {
            let samples = tone_samples(tone).ok_or_else(|| anyhow!("Unknown tone {tone}"))?;
            sink.append(SamplesBuffer::new(1, RATE, samples));
        }
        Sound::File { id } => {
            let bytes = crate::alerts::read(data_dir, id)?;
            sink.append(Decoder::new(Cursor::new(bytes))?.convert_samples::<f32>());
        }
    }
    sink.sleep_until_end();
    Ok(())
}

/// Plays on its own thread, so the caller never waits on the audio device.
pub fn play_async(sound: Sound, volume: f32, data_dir: std::path::PathBuf) {
    std::thread::spawn(move || {
        if let Err(e) = play(&sound, volume, &data_dir) {
            tracing::warn!("alert sound: {e:#}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_are_short_and_never_clip() {
        for t in ["chime", "ping", "knock", "rise", "fall", "soft"] {
            let s = tone_samples(t).unwrap();
            assert!(!s.is_empty() && s.len() < RATE as usize, "{t}");
            assert!(s.iter().all(|x| x.abs() <= 1.0), "{t}");
        }
        assert!(tone_samples("nope").is_none());
    }
}
