//! the console's sound, played on the host's default output device.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

/// how much sound to keep waiting, in the console's samples, about 39 ms,
/// which is how far the sound comes after the picture, less the output's
/// own delay. more rides out uneven frames, less answers sooner: with
/// frames paced to the display 1024 ran dry now and then while playing and
/// 768 often, and 4096, 125 ms, put the sound well behind the picture.
const TARGET: usize = 1280;
/// how much a queue that ran dry waits for before it plays again.
const RESUME: usize = TARGET / 4;
/// the queue stretches sound only once it is down to this, about 20 ms.
/// every stretch splices the sound, so smaller shortfalls are left to DRIFT.
const STRETCH_BELOW: usize = TARGET / 2;
/// how much faster or slower than the rates say playback may go to keep
/// the queue near its target, too little to hear.
const DRIFT: f64 = 0.005;

/// how many of the console's samples go into each output sample, half of
/// them before it and half after.
const TAPS: usize = 64;
/// how many fractions between two samples the filter is worked out for,
/// the rest found between the nearest two.
const PHASES: usize = 256;
/// the Kaiser window's beta, which puts what folds back below the
/// passband at about -90 dB.
const BETA: f64 = 9.0;

/// a windowed sinc filter, the band-limited curve through the console's
/// samples, from which the output takes its samples wherever they fall.
/// a straight line between two samples, as before, dulls the highs and
/// mirrors them back down as a faint hiss; this keeps the band flat to
/// about 15.7 kHz and what lies past it 90 dB down.
struct Kernel {
    /// the weights of the TAPS samples for each of PHASES + 1 fractions,
    /// the output TAPS / 2 - 1 samples in and that fraction on.
    table: Vec<[f32; TAPS]>,
}

impl Kernel {
    /// for step console samples to each output sample.
    fn new(step: f64) -> Kernel {
        // half the slower rate, less a little for the window to fall in,
        // in cycles per console sample
        let cutoff = 0.5 * step.recip().min(1.0) * 0.96;
        let bessel = |x: f64| {
            // I0, the modified Bessel function the window is made of
            let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
            while term > sum * 1e-12 {
                term *= (x / 2.0 / k).powi(2);
                sum += term;
                k += 1.0;
            }
            sum
        };
        let table = (0..=PHASES)
            .map(|phase| {
                let fraction = phase as f64 / PHASES as f64;
                let mut row = [0f64; TAPS];
                for (k, weight) in row.iter_mut().enumerate() {
                    let t = k as f64 - (TAPS / 2 - 1) as f64 - fraction;
                    let x = (t / (TAPS / 2) as f64).clamp(-1.0, 1.0);
                    let window = bessel(BETA * (1.0 - x * x).sqrt()) / bessel(BETA);
                    let arg = std::f64::consts::PI * 2.0 * cutoff * t;
                    let sinc = if arg == 0.0 { 1.0 } else { arg.sin() / arg };
                    *weight = 2.0 * cutoff * sinc * window;
                }
                // the weights add up to one, so a steady level stays
                let sum: f64 = row.iter().sum();
                row.map(|weight| (weight / sum) as f32)
            })
            .collect();
        Kernel { table }
    }

    /// the output fraction of the way from sample TAPS / 2 - 1 to the next.
    fn sample(&self, samples: &VecDeque<[f32; 2]>, fraction: f64) -> [f32; 2] {
        let at = fraction.clamp(0.0, 1.0) * PHASES as f64;
        let phase = (at as usize).min(PHASES - 1);
        let between = (at - phase as f64) as f32;
        let (a, b) = (&self.table[phase], &self.table[phase + 1]);
        let mut out = [0f32; 2];
        for ((sample, wa), wb) in samples.range(..TAPS).zip(a).zip(b) {
            let weight = wa + (wb - wa) * between;
            out[0] += sample[0] * weight;
            out[1] += sample[1] * weight;
        }
        out
    }
}

/// the console's samples waiting to be played.
struct Queue {
    samples: VecDeque<[f32; 2]>,
    /// how far the output is past sample TAPS / 2 - 1 toward the next, the
    /// samples before it there for the filter to look back over.
    fraction: f64,
    kernel: Kernel,
    /// console samples per output sample at 1x speed.
    base_step: f64,
    /// the console's samples a second.
    rate: f64,
    /// speed multiplier for custom FPS limit / uncapped playback.
    speed: f64,
    /// ran dry, and waits to fill up before it plays again.
    starved: bool,
    /// the last sample played, which fades out when the queue runs dry
    /// instead of stopping short.
    last: [f32; 2],
    /// times the queue ran dry.
    underruns: u64,
    /// how much a queue that ran dry waits for before it plays again.
    refill: usize,
    /// when the output last asked for sound, how long after that what it
    /// was given starts playing, and how long that lasts.
    output: Option<(std::time::Instant, std::time::Duration, std::time::Duration)>,
}

impl Queue {
    /// the next output sample, between two of the console's.
    fn next(&mut self) -> [f32; 2] {
        if !self.starved && self.samples.len() < TAPS {
            self.starved = true;
            self.underruns += 1;
        }
        if self.starved {
            if self.samples.len() < self.refill.max(TAPS) {
                self.last = self.last.map(|v| v * 0.995);
                return self.last;
            }
            self.starved = false;
            self.refill = RESUME;
        }
        let out = self.kernel.sample(&self.samples, self.fraction);
        self.last = out;
        // a longer queue plays a little faster, a shorter one slower
        let error = (self.samples.len() as f64 - TARGET as f64) / TARGET as f64;
        let step = self.base_step * self.speed;
        self.fraction += step * (1.0 + error.clamp(-1.0, 1.0) * DRIFT);
        while self.fraction >= 1.0 && self.samples.len() > TAPS {
            self.samples.pop_front();
            self.fraction -= 1.0;
        }
        out
    }

    /// stops it while the game is stopped, without counting that as running
    /// dry, and has it fill all the way up before it plays again, so the
    /// first frames after do not run it dry either.
    fn hold(&mut self) {
        self.starved = true;
        self.refill = TARGET;
        self.underruns = 0;
    }

    /// drops what is queued, holding as above, fading out from what played
    /// last.
    fn clear(&mut self) {
        self.samples.clear();
        self.fraction = 0.0;
        self.hold();
    }
}

/// the pieces of sound stretching moves around, about 16 ms, and how far
/// apart they go out, half of that.
const GRAIN: usize = 512;
const HOP: usize = GRAIN / 2;
/// how far a piece may shift, about 4 ms, to line up with the one before.
const SEARCH: usize = 128;
/// sound gets at most twice as long.
const MOST_STRETCH: f64 = 2.0;

/// time stretching, sound made longer without its pitch changing, for when
/// the emulation falls behind and the queue would run dry. overlapping
/// pieces of it go out further apart than they came in, each shifted to
/// line up with the one before, which is WSOLA.
struct Stretch {
    /// what came in and has not gone out yet.
    input: Vec<[f32; 2]>,
    /// where in input the next piece is due.
    position: f64,
    /// where the last piece came from, none before the first.
    last: Option<usize>,
    /// the last piece's second half, which the next one's first adds to.
    tail: Vec<[f32; 2]>,
    /// how much longer sound comes out, eased toward what the queue asks.
    factor: f64,
    /// not stretching, sound goes straight through, input keeping only
    /// what a stretch starting next would search back over.
    idle: bool,
    window: Vec<f32>,
}

impl Stretch {
    fn new() -> Stretch {
        // a periodic Hann window, whose halves add up to one
        let window = (0..GRAIN).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / GRAIN as f32).cos()).collect();
        Stretch { input: Vec::new(), position: 0.0, last: None, tail: vec![[0.0; 2]; HOP], factor: 1.0, idle: true, window }
    }

    /// how many samples came in that have not gone out yet.
    fn waiting(&self) -> usize {
        if self.idle {
            return 0;
        }
        self.input.len().saturating_sub(self.last.map_or(0, |last| last + HOP))
    }

    /// forgets what came in, to start over on other sound.
    fn clear(&mut self) {
        self.input.clear();
        self.position = 0.0;
        self.last = None;
        self.tail.fill([0.0; 2]);
        self.factor = 1.0;
        self.idle = true;
    }

    /// keeps of what went out only what a stretch would search back over.
    fn keep_history(&mut self) {
        let excess = self.input.len().saturating_sub(HOP + 2 * SEARCH);
        self.input.drain(..excess);
    }

    /// stretches samples by about factor, passing out what is ready.
    fn process(&mut self, samples: &[[f32; 2]], factor: f64, out: &mut Vec<[f32; 2]>) {
        if self.idle {
            let gone = self.input.len();
            self.input.extend_from_slice(samples);
            // a stretch starts from what went out last, which needs some of
            // it to search over and a piece's worth of what is new
            if factor == 1.0 || gone < HOP + SEARCH || samples.len() < HOP {
                out.extend_from_slice(samples);
                self.keep_history();
                return;
            }
            // as if a piece had started a hop before the new sound: its
            // second half, the new sound fading out, goes under the next
            // one's first, so the stretch starts with no dip
            self.idle = false;
            self.last = Some(gone - HOP);
            self.position = gone as f64;
            self.factor = 1.0;
            for ((tail, sample), w) in self.tail.iter_mut().zip(&self.input[gone..gone + HOP]).zip(&self.window[HOP..]) {
                *tail = [sample[0] * w, sample[1] * w];
            }
        } else {
            self.input.extend_from_slice(samples);
        }
        self.factor += (factor - self.factor) * 0.2;
        // close to not stretching any more, it stops
        if factor == 1.0 && self.factor < 1.001 {
            self.factor = 1.0;
        }
        let mono = |s: [f32; 2]| s[0] + s[1];
        loop {
            // not stretching, each piece follows on from the last, with no
            // search and so no splice
            if let Some(last) = self.last.filter(|_| self.factor == 1.0) {
                self.position = (last + HOP) as f64;
            }
            let due = self.position as usize;
            let natural = self.last.map_or(due, |last| last + HOP);
            if due + SEARCH + GRAIN > self.input.len() || natural + GRAIN > self.input.len() {
                break;
            }
            // the piece around where it is due that most looks like the
            // last one's continuation, which is where it is due when
            // nothing is stretched
            let start = if natural == due {
                due
            } else {
                let template = &self.input[natural..natural + HOP];
                let score = |candidate: usize| {
                    let (mut dot, mut energy) = (0.0f32, 1e-9f32);
                    for (a, &b) in self.input[candidate..candidate + HOP].iter().zip(template) {
                        dot += mono(*a) * mono(b);
                        energy += mono(*a) * mono(*a);
                    }
                    dot / energy.sqrt()
                };
                (due.saturating_sub(SEARCH)..=due + SEARCH)
                    .map(|candidate| (score(candidate), candidate))
                    // a tie, as in silence, goes to the piece nearest the
                    // natural one
                    .max_by(|(a, x), (b, y)| a.total_cmp(b).then(natural.abs_diff(*y).cmp(&natural.abs_diff(*x))))
                    .map_or(due, |(_, candidate)| candidate)
            };
            let piece = &self.input[start..start + GRAIN];
            let (first, second) = piece.split_at(HOP);
            let (rising, falling) = self.window.split_at(HOP);
            for ((tail, sample), w) in self.tail.iter_mut().zip(first).zip(rising) {
                out.push([tail[0] + sample[0] * w, tail[1] + sample[1] * w]);
            }
            for ((tail, sample), w) in self.tail.iter_mut().zip(second).zip(falling) {
                *tail = [sample[0] * w, sample[1] * w];
            }
            self.last = Some(start);
            self.position += HOP as f64 / self.factor;
        }
        // done stretching, the last piece's second half and what follows
        // go straight out, which with no stretch is the sound as it came
        if let (1.0, Some(last)) = (self.factor, self.last) {
            let rising = &self.window[..HOP];
            for ((tail, sample), w) in self.tail.iter().zip(&self.input[last + HOP..last + GRAIN]).zip(rising) {
                out.push([tail[0] + sample[0] * w, tail[1] + sample[1] * w]);
            }
            out.extend_from_slice(&self.input[last + GRAIN..]);
            self.idle = true;
            self.last = None;
            self.tail.fill([0.0; 2]);
            self.keep_history();
            return;
        }
        // what no piece can reach any more goes
        let used = (self.position as usize).min(self.last.unwrap_or(0)).saturating_sub(SEARCH);
        if used > 0 {
            self.input.drain(..used);
            self.position -= used as f64;
            self.last = self.last.map(|last| last - used);
        }
    }
}

pub struct Audio {
    queue: Arc<Mutex<Queue>>,
    /// 0 to 1, applied as samples come in.
    volume: std::cell::Cell<f32>,
    stretch: std::cell::RefCell<Stretch>,
    _stream: cpal::Stream,
}

impl Audio {
    /// opens the default output for sound made at rate samples a second.
    pub fn open(rate: f64) -> Result<Audio, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("there is no sound output device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let queue = Arc::new(Mutex::new(Queue {
            samples: VecDeque::new(),
            fraction: 0.0,
            base_step: rate / config.sample_rate.0 as f64,
            kernel: Kernel::new(rate / config.sample_rate.0 as f64),
            rate,
            speed: 1.0,
            starved: true,
            last: [0.0; 2],
            underruns: 0,
            refill: RESUME,
            output: None,
        }));
        let stream = match format {
            SampleFormat::F32 => stream::<f32>(&device, &config, queue.clone()),
            SampleFormat::I16 => stream::<i16>(&device, &config, queue.clone()),
            SampleFormat::U16 => stream::<u16>(&device, &config, queue.clone()),
            SampleFormat::I32 => stream::<i32>(&device, &config, queue.clone()),
            other => return Err(format!("the output takes {other} samples, which Zakuro does not make")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Audio { queue, volume: std::cell::Cell::new(1.0), stretch: std::cell::RefCell::new(Stretch::new()), _stream: stream })
    }

    /// how many times the sound ran dry since the last call.
    pub fn take_underruns(&self) -> u64 {
        self.queue.lock().map(|mut queue| std::mem::take(&mut queue.underruns)).unwrap_or(0)
    }

    /// stops the sound while the game is stopped, see Queue::hold.
    pub fn hold(&self) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.hold();
        }
    }

    /// drops the sound queued and half stretched, for a game starting or
    /// stopping, so none of the last one's plays at the start of the next.
    pub fn clear(&self) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
        }
        self.stretch.borrow_mut().clear();
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume.set(volume.clamp(0.0, 1.0));
    }

    /// how fast the game runs against the console's own rate, slightly
    /// above 1 when its frames are lined up with the display, so the sound
    /// is played as fast as it is made.
    pub fn set_speed(&self, speed: f64) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.speed = speed.clamp(0.1, 10.0);
        }
    }

    /// queues what the console played, stretched when the queue runs low
    /// so that it plays on rather than stops.
    /// returns when the first of them will be heard, as far as the output
    /// says.
    pub fn push(&self, samples: &[[i16; 2]]) -> Option<std::time::Instant> {
        let scale = self.volume.get() / 32768.0;
        let samples: Vec<[f32; 2]> = samples.iter().map(|s| s.map(|v| v as f32 * scale)).collect();
        let Ok(mut queue) = self.queue.lock() else { return None };
        let mut stretch = self.stretch.borrow_mut();
        let heard = heard_at(&queue, &stretch);
        enqueue(&mut queue, &mut stretch, &samples);
        heard
    }
}

/// adds what the console played to the queue, stretched by how far the
/// queue has run down.
fn enqueue(queue: &mut Queue, stretch: &mut Stretch, samples: &[[f32; 2]]) {
    let short = STRETCH_BELOW.saturating_sub(queue.samples.len()) as f64 / STRETCH_BELOW as f64;
    let mut stretched = Vec::with_capacity(samples.len() * 2);
    stretch.process(samples, (1.0 + short).min(MOST_STRETCH), &mut stretched);
    queue.samples.extend(stretched);
    // far ahead, after a stall on the output side, it drops the oldest
    // rather than lag behind the picture
    if queue.samples.len() > TARGET * 4 {
        let excess = queue.samples.len() - TARGET;
        queue.samples.drain(..excess);
    }
}

/// when sound added now will be heard: after what the output was last
/// given, and then what waits in the queue and in the stretcher.
fn heard_at(queue: &Queue, stretch: &Stretch) -> Option<std::time::Instant> {
    let (asked, delay, lasts) = queue.output?;
    let now = std::time::Instant::now();
    let waiting = queue.samples.len() as f64 + stretch.waiting() as f64 - queue.fraction - (TAPS / 2 - 1) as f64;
    // the output goes through the console's rate times speed a second
    let queued = std::time::Duration::from_secs_f64(waiting.max(0.0) / (queue.rate * queue.speed));
    Some((asked + delay + lasts).max(now) + queued)
}

fn stream<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    queue: Arc<Mutex<Queue>>,
) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    let rate = config.sample_rate.0 as f64;
    device
        .build_output_stream(
            config,
            move |out: &mut [T], info: &cpal::OutputCallbackInfo| {
                let Ok(mut queue) = queue.lock() else { return };
                let stamp = info.timestamp();
                let delay = stamp.playback.duration_since(&stamp.callback).unwrap_or_default();
                let lasts = std::time::Duration::from_secs_f64((out.len() / channels.max(1)) as f64 / rate);
                queue.output = Some((std::time::Instant::now(), delay, lasts));
                for frame in out.chunks_mut(channels) {
                    let [left, right] = queue.next();
                    match frame {
                        [mono] => *mono = T::from_sample((left + right) / 2.0),
                        [l, r, rest @ ..] => {
                            *l = T::from_sample(left);
                            *r = T::from_sample(right);
                            rest.fill(T::EQUILIBRIUM);
                        }
                        [] => {}
                    }
                }
            },
            |error| log::warn!("sound output, {error}"),
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue(samples: usize, step: f64) -> Queue {
        Queue {
            samples: (0..samples).map(|i| [i as f32, -(i as f32)]).collect(),
            fraction: 0.0,
            base_step: step,
            kernel: Kernel::new(step),
            rate: 32728.0,
            speed: 1.0,
            starved: false,
            last: [0.0; 2],
            underruns: 0,
            refill: RESUME,
            output: None,
        }
    }

    /// a tone at 440 Hz, the console's rate.
    fn tone(count: usize) -> Vec<[f32; 2]> {
        (0..count)
            .map(|i| {
                let v = (std::f32::consts::TAU * 440.0 * i as f32 / 32728.0).sin() * 0.5;
                [v, v]
            })
            .collect()
    }

    /// how many times a sound crosses zero going up, which tells its pitch.
    fn rises(samples: &[[f32; 2]]) -> usize {
        samples.windows(2).filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0).count()
    }

    #[test]
    fn unstretched_sound_comes_out_as_it_went_in() {
        let input = tone(20_000);
        let mut stretch = Stretch::new();
        let mut out = Vec::new();
        for chunk in input.chunks(546) {
            stretch.process(chunk, 1.0, &mut out);
        }
        // past the first piece fading in, every sample is the one that came in
        assert!(out.len() > 18_000);
        for (a, b) in out[HOP..].iter().zip(&input[HOP..]) {
            assert!((a[0] - b[0]).abs() < 1e-4);
        }
    }

    /// a sound that repeats nowhere, with a stretch of silence in the
    /// middle, so where any piece of it came from is clear.
    fn noise_with_a_pause(count: usize) -> Vec<[f32; 2]> {
        let mut x = 1u32;
        (0..count)
            .map(|i| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let v = if (30_000..33_000).contains(&i) { 0.0 } else { (x >> 8) as f32 / (1 << 24) as f32 - 0.5 };
                [v, v]
            })
            .collect()
    }

    /// a queue a little short of its target is left to DRIFT, its sound goes
    /// in as it came, with no splice.
    #[test]
    fn a_queue_a_little_short_is_not_stretched() {
        let mut queue = queue(TARGET - TARGET / 4, 1.0);
        let mut stretch = Stretch::new();
        let input = noise_with_a_pause(20_000);
        let mut out = Vec::new();
        for chunk in input.chunks(546) {
            let before = queue.samples.len();
            enqueue(&mut queue, &mut stretch, chunk);
            out.extend(queue.samples.range(before..).copied());
            // the output plays as much as came in
            queue.samples.drain(..chunk.len());
        }
        assert!(out.len() > 19_000);
        for (a, b) in out[HOP..].iter().zip(&input[HOP..]) {
            assert!((a[0] - b[0]).abs() < 1e-4);
        }
    }

    /// once the queue is full again the stretcher stops, and the sound
    /// follows on as it came in, silence included, with no splice.
    #[test]
    fn stretching_stops_cleanly() {
        let input = noise_with_a_pause(60_000);
        let mut stretch = Stretch::new();
        let mut out = Vec::new();
        for (i, chunk) in input.chunks(546).enumerate() {
            stretch.process(chunk, if i < 20 { 1.5 } else { 1.0 }, &mut out);
        }
        assert_eq!(stretch.factor, 1.0);
        // where the last of it came from, found in noise, which matches in
        // one place only
        let end = &out[out.len() - 10_000..];
        let from = (0..=input.len() - end.len())
            .find(|&at| end.iter().zip(&input[at..]).take(64).all(|(a, b)| (a[0] - b[0]).abs() < 1e-4))
            .expect("the end is somewhere in the input");
        // and the input in a row back from there, through the end of the pause
        let tail = &out[out.len() - 28_000..];
        let start = from - 18_000;
        assert!(start < 33_000, "it covers the pause");
        for (a, b) in tail.iter().zip(&input[start..]) {
            assert!((a[0] - b[0]).abs() < 1e-4, "spliced");
        }
    }

    /// sound going straight through, then stretched, then straight
    /// through again, keeps its level all the way: no dip where a stretch
    /// starts or stops.
    #[test]
    fn stretches_start_and_stop_without_a_dip() {
        let input = vec![[0.5f32, 0.5]; 546 * 60];
        let mut stretch = Stretch::new();
        let mut out = Vec::new();
        for (i, chunk) in input.chunks(546).enumerate() {
            let factor = if (5..15).contains(&i) { 1.5 } else { 1.0 };
            stretch.process(chunk, factor, &mut out);
            if i == 4 {
                assert_eq!(out.len(), 546 * 5, "nothing held back while not stretching");
            }
        }
        assert!(stretch.idle, "back to going straight through");
        assert!(out.len() > input.len(), "it did stretch");
        for (i, sample) in out.iter().enumerate() {
            assert!((sample[0] - 0.5).abs() < 1e-4, "{} at {i}", sample[0]);
        }
    }

    #[test]
    fn stretched_sound_is_longer_at_the_same_pitch() {
        let input = tone(32_728);
        let mut stretch = Stretch::new();
        let mut out = Vec::new();
        for chunk in input.chunks(546) {
            stretch.process(chunk, 1.5, &mut out);
        }
        let ratio = out.len() as f64 / input.len() as f64;
        assert!(ratio > 1.35 && ratio < 1.55, "stretched by {ratio}");
        // the same number of cycles a second
        let pitch = rises(&out) as f64 / out.len() as f64 * 32728.0;
        assert!((pitch - 440.0).abs() < 10.0, "pitch {pitch}");
    }

    #[test]
    fn output_falls_between_the_consoles_samples() {
        // a ramp comes out a ramp, the filter looking TAPS / 2 - 1 back
        let mut queue = queue(TARGET, 0.5);
        let start = (TAPS / 2 - 1) as f32;
        for expected in [start, start + 0.5, start + 1.0, start + 1.5] {
            let [left, right] = queue.next();
            assert!((left - expected).abs() < 0.01 && (right + expected).abs() < 0.01, "{left} for {expected}");
        }
    }

    /// how far off a tone at frequency comes out of the console's rate at
    /// 48 kHz, against the exact curve, in dB below the tone.
    fn resampling_error(frequency: f64) -> f64 {
        const CONSOLE: f64 = 32728.0;
        let step = CONSOLE / 48000.0;
        let wave = |at: f64| (std::f64::consts::TAU * frequency * at / CONSOLE).sin() * 0.5;
        let mut queue = queue(0, step);
        queue.samples.extend((0..20_000).map(|i| [wave(i as f64) as f32; 2]));
        let (mut signal, mut error) = (0.0, 0.0);
        for _ in 0..20_000 {
            // where the output falls, TAPS / 2 - 1 past what was played
            // through, which drift control moves a little off the step
            let at = (20_000 - queue.samples.len() + TAPS / 2 - 1) as f64 + queue.fraction;
            let out = queue.next()[0] as f64;
            let exact = wave(at);
            signal += exact * exact;
            error += (out - exact) * (out - exact);
        }
        10.0 * (signal / error).log10()
    }

    #[test]
    fn tones_come_through_clean() {
        for (frequency, least) in [(440.0, 95.0), (3000.0, 95.0), (10_000.0, 95.0), (14_000.0, 80.0)] {
            let db = resampling_error(frequency);
            eprintln!("{frequency} Hz: {db:.1} dB");
            assert!(db > least, "{frequency} Hz is {db:.1} dB clean, wanted {least}");
        }
    }

    #[test]
    fn a_queue_that_ran_dry_fades_and_waits_to_fill_up() {
        let mut queue = queue(1, 1.0);
        queue.last = [0.5, 0.5];
        let faded = queue.next();
        assert!(queue.starved);
        assert_eq!(queue.underruns, 1);
        assert!(faded[0] < 0.5 && faded[0] > 0.4, "it fades out rather than stopping short");
        queue.samples.extend((0..RESUME.max(TAPS)).map(|_| [1.0, 1.0]));
        assert!((queue.next()[0] - 1.0).abs() < 0.01, "it plays again once there is enough");
    }

    /// stopped while the game is, it does not count running dry, and plays
    /// again only once it is full.
    #[test]
    fn a_held_queue_waits_to_fill_up_and_counts_nothing() {
        let mut queue = queue(0, 1.0);
        queue.last = [0.5, 0.5];
        queue.hold();
        assert!(queue.next()[0] < 0.5);
        assert_eq!(queue.underruns, 0);
        queue.samples.extend((0..RESUME).map(|_| [1.0, 1.0]));
        assert!(queue.next()[0] < 0.5, "what is enough after running dry is not after a hold");
        queue.samples.extend((0..TARGET).map(|_| [1.0, 1.0]));
        assert_eq!(queue.next(), [1.0, 1.0]);
        assert_eq!(queue.underruns, 0);
        // running dry later is counted, and waits for the usual amount
        queue.samples.clear();
        queue.next();
        assert_eq!(queue.underruns, 1);
        queue.samples.extend((0..RESUME).map(|_| [1.0, 1.0]));
        assert_eq!(queue.next(), [1.0, 1.0]);
    }

    /// a game starting or stopping drops what the last one queued, and the
    /// new sound plays once there is enough of it.
    #[test]
    fn a_cleared_queue_plays_none_of_the_old_sound() {
        let mut queue = queue(TARGET, 1.0);
        queue.clear();
        queue.next();
        queue.samples.extend((0..TARGET).map(|_| [-1.0, -1.0]));
        assert_eq!(queue.next(), [-1.0, -1.0]);
        assert_eq!(queue.underruns, 0);
    }

    /// how far behind the picture the sound is, in ms, after each second of
    /// a minute where the console makes exactly as much sound as the output
    /// plays, but a frame now and then comes late and the next ones catch up,
    /// as when the emulation hitches.
    fn latency_with_hitches() -> Vec<f64> {
        const CONSOLE: f64 = 32728.498;
        const OUTPUT: f64 = 48000.0;
        let mut queue = queue(0, CONSOLE / OUTPUT);
        let mut stretch = Stretch::new();
        let mut seconds = Vec::new();
        let tone = tone(40_000);
        let mut at = 0;
        let mut late: Vec<[f32; 2]> = Vec::new();
        for frame in 0..3600usize {
            // a frame's worth of sound, held back every 45th frame for three
            // frames' time
            let count = (CONSOLE * (frame + 1) as f64 / 60.0) as usize - (CONSOLE * frame as f64 / 60.0) as usize;
            let mut chunk: Vec<[f32; 2]> = (0..count).map(|i| tone[(at + i) % tone.len()]).collect();
            at += count;
            if frame % 45 == 0 {
                late.append(&mut chunk);
            } else if frame % 45 == 3 {
                late.append(&mut chunk);
                enqueue(&mut queue, &mut stretch, &late);
                late.clear();
            } else if late.is_empty() {
                enqueue(&mut queue, &mut stretch, &chunk);
            } else {
                late.append(&mut chunk);
            }
            for _ in 0..(OUTPUT / 60.0) as usize {
                queue.next();
            }
            if frame % 60 == 59 {
                seconds.push(queue.samples.len() as f64 / CONSOLE * 1000.0);
            }
        }
        seconds
    }

    #[test]
    fn hitches_do_not_put_the_sound_further_and_further_behind() {
        let seconds = latency_with_hitches();
        let first = seconds[5];
        let last = *seconds.last().unwrap();
        let most = seconds.iter().cloned().fold(0.0, f64::max);
        eprintln!("latency by second {:?}", seconds.iter().map(|ms| ms.round() as i64).collect::<Vec<_>>());
        assert!(last < first + 30.0, "behind by {first:.0} ms after 5 s and {last:.0} ms after a minute");
        assert!(most < 160.0, "up to {most:.0} ms behind");
    }

    #[test]
    fn a_long_queue_plays_a_little_faster() {
        let mut long = queue(TARGET * 2, 1.0);
        let mut short = queue(TARGET, 1.0);
        for _ in 0..1000 {
            long.next();
            short.next();
        }
        assert!(TARGET * 2 - long.samples.len() > 1000, "the long queue went through more than it played");
        assert!(TARGET - short.samples.len() < 1000, "the short queue went through less than it played");
    }
}
