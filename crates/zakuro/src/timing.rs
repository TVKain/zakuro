//! a record of when frames were emulated, shown and heard, for measuring
//! input lag, how evenly frames reach the screen and how far the sound is
//! from the picture. ZAKURO_TIMING=<file> writes it: a first line
//! `F <ticks a second>`, then for each emulated frame
//! `E <started> <finished> <due> <frame>`, started being when its input was
//! read, for each present `P <started> <finished> <frame> <vblank>
//! <refresh>`, frame the newest emulated one it shows, vblank the display's
//! last vertical blank and refresh its period, both from Windows' compositor
//! (0 where unknown), and for each frame's sound `A <added> <heard> <frame>`,
//! heard when its first sample is due out of the speakers, as far as the
//! output says. times are in ticks of the system's performance counter.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

pub struct Timing {
    out: BufWriter<File>,
}

impl Timing {
    /// the record ZAKURO_TIMING names, if it does.
    pub fn from_env() -> Option<Timing> {
        let path = PathBuf::from(std::env::var_os("ZAKURO_TIMING")?);
        let file = match File::create(&path) {
            Ok(file) => file,
            Err(error) => {
                log::warn!("could not write timing to {}, {error}", path.display());
                return None;
            }
        };
        let mut out = BufWriter::new(file);
        let _ = writeln!(out, "F {}", frequency());
        log::info!("writing timing to {}", path.display());
        Some(Timing { out })
    }

    /// a frame emulated from started to now, which was due at due.
    pub fn emulated(&mut self, started: u64, due: Instant, frame: u64) {
        let finished = ticks();
        let _ = writeln!(self.out, "E {started} {finished} {} {frame}", at(due, finished));
        let _ = self.out.flush();
    }

    /// a frame's sound, added now, first heard at heard.
    pub fn heard(&mut self, heard: Instant, frame: u64) {
        let now = ticks();
        let _ = writeln!(self.out, "A {now} {} {frame}", at(heard, now));
    }

    /// a present from started to now, showing up to frame.
    pub fn presented(&mut self, started: u64, frame: u64) {
        let finished = ticks();
        let (vblank, refresh) = compositor();
        let _ = writeln!(self.out, "P {started} {finished} {frame} {vblank} {refresh}");
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        let _ = self.out.flush();
    }
}

/// when instant was, in ticks, given that now is ticks now.
fn at(instant: Instant, now: u64) -> u64 {
    let since = Instant::now().saturating_duration_since(instant).as_secs_f64();
    let until = instant.saturating_duration_since(Instant::now()).as_secs_f64();
    let frequency = frequency() as f64;
    (now as f64 - since * frequency + until * frequency) as u64
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryPerformanceCounter(count: *mut i64) -> i32;
    fn QueryPerformanceFrequency(frequency: *mut i64) -> i32;
}

/// now, in ticks of the performance counter Windows' compositor counts in.
#[cfg(windows)]
pub fn ticks() -> u64 {
    let mut count = 0i64;
    // SAFETY: writes the one integer it is given
    unsafe { QueryPerformanceCounter(&mut count) };
    count as u64
}

#[cfg(windows)]
pub fn frequency() -> u64 {
    let mut frequency = 0i64;
    // SAFETY: writes the one integer it is given
    unsafe { QueryPerformanceFrequency(&mut frequency) };
    frequency as u64
}

#[cfg(not(windows))]
pub fn ticks() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

#[cfg(not(windows))]
pub fn frequency() -> u64 {
    1_000_000_000
}

/// DWM_TIMING_INFO, packed as dwmapi.h declares it.
#[cfg(windows)]
#[repr(C, packed)]
struct DwmTimingInfo {
    size: u32,
    rate_refresh: [u32; 2],
    qpc_refresh_period: u64,
    rate_compose: [u32; 2],
    qpc_vblank: u64,
    rest: [u8; 256],
}

/// the display's last vertical blank and its period, in ticks, as Windows'
/// compositor has them.
#[cfg(windows)]
pub fn compositor() -> (u64, u64) {
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmGetCompositionTimingInfo(window: isize, info: *mut DwmTimingInfo) -> i32;
    }
    // SAFETY: plain integers, for which zero is a value
    let mut info: DwmTimingInfo = unsafe { std::mem::zeroed() };
    info.size = std::mem::size_of::<DwmTimingInfo>() as u32;
    // SAFETY: the struct is the size given, 292 bytes
    let result = unsafe { DwmGetCompositionTimingInfo(0, &mut info) };
    if result != 0 {
        return (0, 0);
    }
    (info.qpc_vblank, info.qpc_refresh_period)
}

#[cfg(not(windows))]
pub fn compositor() -> (u64, u64) {
    (0, 0)
}

/// the display's last refresh and how often it refreshes, if the
/// compositor says.
pub fn vblank() -> Option<(Instant, std::time::Duration)> {
    let (vblank, period) = compositor();
    let refresh = refresh_of(period)?;
    let (now, ticks) = (Instant::now(), ticks());
    let seconds = |n: u64| std::time::Duration::from_secs_f64(n as f64 / frequency() as f64);
    // the compositor's refresh can be a little ahead of now
    let at = if ticks >= vblank { now.checked_sub(seconds(ticks - vblank))? } else { now + seconds(vblank - ticks) };
    Some((at, refresh))
}

/// how often the display refreshes, if the compositor says.
pub fn refresh() -> Option<std::time::Duration> {
    refresh_of(compositor().1)
}

fn refresh_of(period: u64) -> Option<std::time::Duration> {
    let seconds = period as f64 / frequency() as f64;
    // anything outside 24 to 1000 Hz is not a display's
    (0.001..=1.0 / 24.0).contains(&seconds).then(|| std::time::Duration::from_secs_f64(seconds))
}
