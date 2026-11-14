//! lines frames up with the display. the console makes 59.83 frames a
//! second and a 60 or 120 Hz display shows 60, so left to themselves frames
//! drift across its refreshes: one in a while waits a refresh longer than
//! the rest, and how long any waits for its refresh is down to chance. here
//! each frame starts just early enough to be ready a little before a
//! refresh, which shows every frame for the same number of refreshes and
//! cuts the wait to that little. the game then runs at the display's rate,
//! 0.3% faster at 60 Hz, which the sound follows.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// how many frames' work the estimate of the next one's looks back over,
/// two seconds.
const LOOK_BACK: usize = 120;
/// how long before its refresh a frame aims to be ready, for the
/// compositor to take it.
const MARGIN: Duration = Duration::from_micros(1500);
/// how far one frame's start may move, so a jump in the work or a wrong
/// refresh moves the game's time only a little.
const MOST_NUDGE: Duration = Duration::from_micros(500);
/// how far the display's rate may be from the console's, a whole number of
/// refreshes a frame, for frames to line up with it.
const MOST_MISMATCH: f64 = 0.01;

#[derive(Default)]
pub struct Pacing {
    /// how long recent frames took from reading input to being shown.
    work: VecDeque<Duration>,
}

impl Pacing {
    /// a frame that took work from reading input to being shown.
    pub fn record(&mut self, work: Duration) {
        if self.work.len() == LOOK_BACK {
            self.work.pop_front();
        }
        self.work.push_back(work);
    }

    /// forgets the work, for another game or another kind of frame.
    pub fn clear(&mut self) {
        self.work.clear();
    }

    /// how long the next frame may take: the longest lately.
    fn estimate(&self) -> Option<Duration> {
        self.work.iter().max().copied()
    }

    /// when a frame due at due should start instead, given the display's
    /// last refresh at vblank and its period.
    pub fn align(&self, due: Instant, frame: Duration, vblank: Instant, refresh: Duration) -> Instant {
        match (self.estimate(), refreshes(frame, refresh)) {
            (Some(work), Some(_)) => nudge(due, work, vblank, refresh),
            _ => due,
        }
    }
}

/// how many refreshes a frame lasts when the display's rate is close enough
/// to a whole number of them for frames to line up with it.
pub fn refreshes(frame: Duration, refresh: Duration) -> Option<u32> {
    if refresh.is_zero() {
        return None;
    }
    let ratio = frame.as_secs_f64() / refresh.as_secs_f64();
    let whole = ratio.round();
    (whole >= 1.0 && (ratio - whole).abs() / ratio <= MOST_MISMATCH).then_some(whole as u32)
}

/// how fast the game runs lined up with the display, its frames lasting a
/// whole number of refreshes, against the console's own rate.
pub fn speed(frame: Duration, refresh: Duration) -> f64 {
    match refreshes(frame, refresh) {
        Some(n) => frame.as_secs_f64() / (refresh.as_secs_f64() * n as f64),
        None => 1.0,
    }
}

/// due moved toward the start that has a frame taking work ready MARGIN
/// before a refresh, the nearest such, by MOST_NUDGE at most.
fn nudge(due: Instant, work: Duration, vblank: Instant, refresh: Duration) -> Instant {
    let ready = due + work + MARGIN;
    let period = refresh.as_secs_f64();
    let since = if ready >= vblank {
        ready.duration_since(vblank).as_secs_f64()
    } else {
        -vblank.duration_since(ready).as_secs_f64()
    };
    // how far past its nearest refresh the frame would be ready
    let past = since - (since / period).round() * period;
    let most = MOST_NUDGE.as_secs_f64();
    let shift = (-past).clamp(-most, most);
    if shift >= 0.0 {
        due + Duration::from_secs_f64(shift)
    } else {
        due - Duration::from_secs_f64(-shift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: f64) -> Duration {
        Duration::from_secs_f64(n / 1000.0)
    }

    #[test]
    fn lines_up_with_displays_close_to_the_consoles_rate() {
        let frame = ms(16.7137);
        assert_eq!(refreshes(frame, ms(1000.0 / 60.0)), Some(1));
        assert_eq!(refreshes(frame, ms(1000.0 / 120.0)), Some(2));
        assert_eq!(refreshes(frame, ms(1000.0 / 240.0)), Some(4));
        assert_eq!(refreshes(frame, ms(1000.0 / 59.94)), Some(1));
        assert_eq!(refreshes(frame, ms(1000.0 / 144.0)), None);
        assert_eq!(refreshes(frame, ms(1000.0 / 75.0)), None);
        assert_eq!(refreshes(frame, Duration::ZERO), None);
        assert!((speed(frame, ms(1000.0 / 60.0)) - 1.0029).abs() < 0.0001);
        assert_eq!(speed(frame, ms(1000.0 / 144.0)), 1.0);
    }

    #[test]
    fn frames_move_toward_being_ready_just_before_a_refresh() {
        let t = Instant::now();
        let refresh = ms(1000.0 / 60.0);
        let at = |n: f64| t + ms(n);
        let off = |a: Instant, b: Instant| (a.duration_since(t).as_secs_f64() - b.duration_since(t).as_secs_f64()) * 1000.0;
        // ready at 10 + 4 + 1.5 = 15.5, the refresh at 16.67: 0.5 later
        assert!((off(nudge(at(10.0), ms(4.0), at(0.0), refresh), at(10.0)) - 0.5).abs() < 1e-6);
        // ready at 17.5, just past it: 0.5 earlier
        assert!((off(nudge(at(12.0), ms(4.0), at(0.0), refresh), at(12.0)) + 0.5).abs() < 1e-6);
        // ready 0.2 before it: 0.2 later, and then it stays
        let due = nudge(at(11.0 - 0.2 + 0.1667), ms(4.0), at(0.0), refresh);
        assert!((off(due, at(11.1667)) - 0.0).abs() < 0.01);
        assert!(off(nudge(due, ms(4.0), at(0.0), refresh), due).abs() < 1e-3);
        // a refresh far in the past or the future is the same grid
        assert!(off(nudge(due, ms(4.0), t - ms(1000.0), refresh), due).abs() < 1e-3);
    }

    #[test]
    fn the_estimate_is_the_longest_recent_frame() {
        let mut pacing = Pacing::default();
        let t = Instant::now();
        assert_eq!(pacing.align(t, ms(16.7137), t, ms(1000.0 / 60.0)), t, "nothing to go on yet");
        for n in 0..200 {
            pacing.record(ms(if n == 10 { 9.0 } else if n % 2 == 0 { 4.0 } else { 1.0 }));
        }
        assert_eq!(pacing.estimate(), Some(ms(4.0)), "the long one went out of the window");
        assert_eq!(pacing.align(t, ms(16.7137), t, ms(1000.0 / 144.0)), t, "not lined up with 144 Hz");
    }
}
