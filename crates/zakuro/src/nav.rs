//! one way of moving around the interface, whatever it is done with: the
//! keyboard's arrows and the controller's d-pad, stick and face buttons all
//! turn into the same few presses, and which of them, or the mouse, was used
//! last decides what the interface shows.

use std::time::{Duration, Instant};

use gilrs::Button;
use winit::keyboard::KeyCode;

/// a press that moves around the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    /// A, Enter or Space: does what is selected.
    Accept,
    /// B, Escape or Backspace: goes back a step.
    Back,
    /// X: the selection's second action, putting a game's option back to
    /// the system's say.
    X,
    /// Y: the selection's options.
    Y,
    /// LB, RB, Q and E: the section before or after.
    PrevTab,
    NextTab,
    /// Start or +: the system settings.
    Start,
}

impl Nav {
    /// the press a key makes, none for keys that don't move around.
    pub fn from_key(code: KeyCode) -> Option<Nav> {
        Some(match code {
            KeyCode::ArrowUp | KeyCode::Numpad8 => Nav::Up,
            KeyCode::ArrowDown | KeyCode::Numpad2 => Nav::Down,
            KeyCode::ArrowLeft | KeyCode::Numpad4 => Nav::Left,
            KeyCode::ArrowRight | KeyCode::Numpad6 => Nav::Right,
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => Nav::Accept,
            KeyCode::Escape | KeyCode::Backspace => Nav::Back,
            KeyCode::KeyX | KeyCode::Delete => Nav::X,
            KeyCode::KeyY => Nav::Y,
            KeyCode::KeyQ | KeyCode::PageUp => Nav::PrevTab,
            KeyCode::KeyE | KeyCode::PageDown | KeyCode::Tab => Nav::NextTab,
            _ => return None,
        })
    }

    /// the press a controller button makes, by its place on the pad: the
    /// bottom face button accepts and the right one goes back, as on an
    /// Xbox pad and the handhelds built like one.
    pub fn from_button(button: Button) -> Option<Nav> {
        Some(match button {
            Button::DPadUp => Nav::Up,
            Button::DPadDown => Nav::Down,
            Button::DPadLeft => Nav::Left,
            Button::DPadRight => Nav::Right,
            Button::South => Nav::Accept,
            Button::East => Nav::Back,
            Button::West => Nav::X,
            Button::North => Nav::Y,
            Button::LeftTrigger | Button::LeftTrigger2 => Nav::PrevTab,
            Button::RightTrigger | Button::RightTrigger2 => Nav::NextTab,
            Button::Start => Nav::Start,
            _ => return None,
        })
    }

    /// whether it moves the selection, which a held key or button repeats.
    pub fn repeats(self) -> bool {
        matches!(self, Nav::Up | Nav::Down | Nav::Left | Nav::Right)
    }
}

/// what the player used last, which the interface follows: with the mouse
/// or a finger, what is under it is highlighted and the button hints go;
/// with a controller or the arrows, a highlight moves from item to item and
/// the hints show. It starts directional, so that a resting pointer picks
/// nothing until the mouse is moved: on a handheld the hidden cursor could
/// otherwise sit over Quit when A is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    Pointer,
    #[default]
    Directional,
}

/// how long a direction is held before it starts repeating, and how often
/// it repeats then.
const REPEAT_DELAY: Duration = Duration::from_millis(380);
const REPEAT_EVERY: Duration = Duration::from_millis(85);

/// repeats a direction held on a controller, which gives one press when it
/// goes down, the way a held key repeats.
#[derive(Default)]
pub struct Repeat {
    held: Option<(Button, Instant)>,
}

impl Repeat {
    /// a direction went down, or one already held repeats from now.
    pub fn pressed(&mut self, button: Button, now: Instant) {
        if Nav::from_button(button).is_some_and(Nav::repeats) {
            self.held = Some((button, now + REPEAT_DELAY));
        }
    }

    /// the repeats due by now, while holding says the direction is still
    /// held.
    pub fn due(&mut self, now: Instant, holding: impl Fn(Button) -> bool) -> Option<Button> {
        let (button, next) = self.held?;
        if !holding(button) {
            self.held = None;
            return None;
        }
        if now < next {
            return None;
        }
        self.held = Some((button, now + REPEAT_EVERY));
        Some(button)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_and_controller_make_the_same_presses() {
        assert_eq!(Nav::from_key(KeyCode::ArrowUp), Nav::from_button(Button::DPadUp));
        assert_eq!(Nav::from_key(KeyCode::Enter), Nav::from_button(Button::South));
        assert_eq!(Nav::from_key(KeyCode::Escape), Nav::from_button(Button::East));
        assert_eq!(Nav::from_key(KeyCode::KeyQ), Nav::from_button(Button::LeftTrigger));
        assert_eq!(Nav::from_key(KeyCode::KeyA), None);
        assert_eq!(Nav::from_button(Button::Mode), None);
    }

    #[test]
    fn a_held_direction_repeats_after_a_pause() {
        let start = Instant::now();
        let mut repeat = Repeat::default();
        repeat.pressed(Button::DPadDown, start);
        assert_eq!(repeat.due(start + Duration::from_millis(100), |_| true), None);
        let first = start + REPEAT_DELAY;
        assert_eq!(repeat.due(first, |_| true), Some(Button::DPadDown));
        assert_eq!(repeat.due(first + Duration::from_millis(10), |_| true), None);
        assert_eq!(repeat.due(first + REPEAT_EVERY, |_| true), Some(Button::DPadDown));
        // let go, it stops
        assert_eq!(repeat.due(first + REPEAT_EVERY * 3, |_| false), None);
        assert_eq!(repeat.due(first + REPEAT_EVERY * 4, |_| true), None);
    }

    #[test]
    fn a_resting_pointer_picks_nothing_at_start() {
        assert_eq!(InputMode::default(), InputMode::Directional);
    }

    #[test]
    fn only_directions_repeat() {
        let start = Instant::now();
        let mut repeat = Repeat::default();
        repeat.pressed(Button::South, start);
        assert_eq!(repeat.due(start + Duration::from_secs(2), |_| true), None);
    }
}
