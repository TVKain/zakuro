//! the interface's own sounds: short, soft tones made here as they are
//! needed, played on an output of their own, apart from the game's sound,
//! and only while no game runs.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

/// what happened in the interface, each with its own sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// the selection moved.
    Move,
    /// something was picked.
    Accept,
    /// went back, or closed something.
    Back,
    /// a switch flipped.
    Toggle,
    /// a message came up.
    Notice,
}

impl Cue {
    const ALL: [Cue; 5] = [Cue::Move, Cue::Accept, Cue::Back, Cue::Toggle, Cue::Notice];
}

/// a tone of the sound: its pitch, gliding to a second one, when it starts
/// and how long it lasts, in seconds, and how loud it is at most.
struct Tone {
    from: f32,
    to: f32,
    start: f32,
    length: f32,
    level: f32,
}

const fn tone(from: f32, to: f32, start: f32, length: f32, level: f32) -> Tone {
    Tone { from, to, start, length, level }
}

const MOVE: &[Tone] = &[tone(1760.0, 1600.0, 0.0, 0.028, 0.10)];
const ACCEPT: &[Tone] = &[tone(880.0, 880.0, 0.0, 0.05, 0.14), tone(1318.5, 1318.5, 0.045, 0.08, 0.14)];
const BACK: &[Tone] = &[tone(1046.5, 1046.5, 0.0, 0.05, 0.12), tone(698.5, 698.5, 0.045, 0.08, 0.12)];
const TOGGLE: &[Tone] = &[tone(1396.9, 1396.9, 0.0, 0.025, 0.10), tone(1864.7, 1864.7, 0.022, 0.035, 0.10)];
const NOTICE: &[Tone] = &[tone(659.3, 659.3, 0.0, 0.32, 0.10), tone(987.8, 987.8, 0.06, 0.32, 0.09), tone(1318.5, 1318.5, 0.12, 0.30, 0.06)];

/// the tones a cue is made of, quiet, as the interface's sounds should be.
fn tones(cue: Cue) -> &'static [Tone] {
    match cue {
        Cue::Move => MOVE,
        Cue::Accept => ACCEPT,
        Cue::Back => BACK,
        Cue::Toggle => TOGGLE,
        Cue::Notice => NOTICE,
    }
}

/// a cue's samples at rate a second: each tone a sine with a little of its
/// octave, rising in a few milliseconds and dying away, so none clicks.
pub fn render(cue: Cue, rate: f32) -> Vec<f32> {
    let tones = tones(cue);
    let end = tones.iter().map(|tone| tone.start + tone.length).fold(0.0, f32::max);
    let mut out = vec![0.0f32; (end * rate).ceil() as usize + 1];
    for tone in tones {
        let first = (tone.start * rate) as usize;
        let count = (tone.length * rate) as usize;
        let mut phase = 0.0f32;
        for i in 0..count {
            let t = i as f32 / rate;
            let along = i as f32 / count as f32;
            let pitch = tone.from + (tone.to - tone.from) * along;
            phase = (phase + pitch / rate).fract();
            let attack = (t / 0.003).min(1.0);
            // falls to near nothing by the tone's end
            let decay = (-5.0 * along).exp() * (1.0 - along);
            let wave = (phase * std::f32::consts::TAU).sin() + 0.18 * (phase * 2.0 * std::f32::consts::TAU).sin();
            if let Some(sample) = out.get_mut(first + i) {
                *sample += wave * attack * decay * tone.level;
            }
        }
    }
    out
}

/// the cues playing, and how far along each is.
#[derive(Default)]
struct Mixer {
    sounds: Vec<Vec<f32>>,
    playing: Vec<(usize, usize, f32)>,
}

impl Mixer {
    /// starts the sound at index, cutting short the same one playing when
    /// cut, with no more than eight at once.
    fn start(&mut self, index: usize, volume: f32, cut: bool) {
        if cut {
            self.playing.retain(|(sound, _, _)| *sound != index);
        }
        if self.playing.len() < 8 {
            self.playing.push((index, 0, volume.min(1.0)));
        }
    }

    fn next(&mut self) -> f32 {
        let mut sum = 0.0;
        let sounds = &self.sounds;
        self.playing.retain_mut(|(sound, at, volume)| {
            let Some(samples) = sounds.get(*sound) else { return false };
            let Some(sample) = samples.get(*at) else { return false };
            sum += sample * *volume;
            *at += 1;
            true
        });
        sum.clamp(-1.0, 1.0)
    }
}

/// plays the cues, opening the output the first time one is played.
#[derive(Default)]
pub struct UiSound {
    mixer: Arc<Mutex<Mixer>>,
    stream: Option<cpal::Stream>,
    /// the output couldn't be opened, and isn't tried again.
    failed: bool,
}

impl UiSound {
    /// plays cue at volume, 0 to 1. a move cuts short the moves before it,
    /// so holding a direction doesn't pile them up.
    pub fn play(&mut self, cue: Cue, volume: f32) {
        if volume <= 0.0 || !self.ready() {
            return;
        }
        let Ok(mut mixer) = self.mixer.lock() else { return };
        let index = Cue::ALL.iter().position(|each| *each == cue).unwrap_or(0);
        mixer.start(index, volume, cue == Cue::Move);
    }

    /// lets go of the output, as a game starting has its own.
    pub fn close(&mut self) {
        self.stream = None;
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.playing.clear();
        }
    }

    fn ready(&mut self) -> bool {
        if self.stream.is_some() {
            return true;
        }
        if self.failed {
            return false;
        }
        match self.open() {
            Ok(stream) => {
                self.stream = Some(stream);
                true
            }
            Err(error) => {
                log::warn!("interface sounds are off, {error}");
                self.failed = true;
                false
            }
        }
    }

    fn open(&mut self) -> Result<cpal::Stream, String> {
        let device = cpal::default_host().default_output_device().ok_or("there is no sound output device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate.0 as f32;
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.sounds = Cue::ALL.iter().map(|cue| render(*cue, rate)).collect();
        }
        let mixer = self.mixer.clone();
        let stream = match format {
            SampleFormat::F32 => stream::<f32>(&device, &config, mixer),
            SampleFormat::I16 => stream::<i16>(&device, &config, mixer),
            SampleFormat::U16 => stream::<u16>(&device, &config, mixer),
            SampleFormat::I32 => stream::<i32>(&device, &config, mixer),
            other => return Err(format!("the output takes {other} samples")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(stream)
    }
}

fn stream<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: &cpal::StreamConfig, mixer: Arc<Mutex<Mixer>>) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _| {
                let Ok(mut mixer) = mixer.lock() else { return };
                for frame in out.chunks_mut(channels) {
                    let sample = T::from_sample(mixer.next());
                    frame.fill(sample);
                }
            },
            |error| log::warn!("interface sound output, {error}"),
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_are_short_quiet_and_end_in_silence() {
        for cue in Cue::ALL {
            let samples = render(cue, 48000.0);
            assert!(samples.len() < 48000 / 2, "{cue:?} is under half a second");
            let loudest = samples.iter().fold(0.0f32, |most, s| most.max(s.abs()));
            assert!(loudest > 0.02 && loudest < 0.35, "{cue:?} peaks at {loudest}");
            assert!(samples[0].abs() < 0.01, "{cue:?} starts without a click");
            assert!(samples.last().unwrap().abs() < 0.01, "{cue:?} ends without a click");
        }
    }

    #[test]
    fn a_move_cuts_the_one_before_short() {
        let mut mixer = Mixer { sounds: vec![vec![0.1; 100], vec![0.2; 100]], playing: Vec::new() };
        mixer.start(0, 1.0, true);
        mixer.start(0, 1.0, true);
        assert!((mixer.next() - 0.1).abs() < 1e-6, "one move plays, not two");
        mixer.start(1, 0.5, false);
        mixer.start(1, 0.5, false);
        assert!((mixer.next() - 0.3).abs() < 1e-6, "other cues overlap");
    }
}
