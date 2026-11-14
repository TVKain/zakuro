//! game controllers, read through gilrs as Zakuro reads them, turned into the
//! few presses setup's pages need: a direction, A, and B.

use std::time::{Duration, Instant};

use gilrs::{Axis, Button, EventType, Gilrs};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Press {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Back,
}

/// how far a stick goes before it counts as a direction, and how far back
/// before it lets go of it.
const STICK_ON: f32 = 0.55;
const STICK_OFF: f32 = 0.25;
/// a direction held goes again after this, then every REPEAT.
const DELAY: Duration = Duration::from_millis(400);
const REPEAT: Duration = Duration::from_millis(130);

pub struct Pad {
    gilrs: Option<Gilrs>,
    /// the D-pad direction held, and the stick's.
    dpad: Option<Press>,
    stick: Option<Press>,
    stick_x: f32,
    stick_y: f32,
    /// when the direction held goes next.
    next: Option<Instant>,
}

impl Pad {
    pub fn new() -> Pad {
        Pad { gilrs: Gilrs::new().ok(), dpad: None, stick: None, stick_x: 0.0, stick_y: 0.0, next: None }
    }

    /// whether a controller is there, which then needs reading every frame.
    pub fn connected(&self) -> bool {
        self.gilrs.as_ref().is_some_and(|gilrs| gilrs.gamepads().next().is_some())
    }

    /// the presses since the last call.
    pub fn presses(&mut self) -> Vec<Press> {
        let mut presses = Vec::new();
        let Some(gilrs) = &mut self.gilrs else { return presses };
        let held_before = self.dpad.or(self.stick);
        while let Some(event) = gilrs.next_event() {
            match event.event {
                EventType::ButtonPressed(button, _) => match button {
                    Button::South => presses.push(Press::Accept),
                    Button::East => presses.push(Press::Back),
                    _ => {
                        if let Some(direction) = dpad(button) {
                            self.dpad = Some(direction);
                            presses.push(direction);
                            self.next = Some(Instant::now() + DELAY);
                        }
                    }
                },
                EventType::ButtonReleased(button, _) if dpad(button).is_some() && dpad(button) == self.dpad => self.dpad = None,
                EventType::AxisChanged(Axis::LeftStickX, value, _) => self.stick_x = value,
                // gilrs has up positive
                EventType::AxisChanged(Axis::LeftStickY, value, _) => self.stick_y = -value,
                EventType::Disconnected => {
                    self.dpad = None;
                    self.stick_x = 0.0;
                    self.stick_y = 0.0;
                }
                _ => {}
            }
        }
        if let Some(direction) = stick(self.stick, self.stick_x, self.stick_y) {
            if self.stick != Some(direction) {
                presses.push(direction);
                self.next = Some(Instant::now() + DELAY);
            }
            self.stick = Some(direction);
        } else if self.stick.is_some() && self.stick_x.abs().max(self.stick_y.abs()) < STICK_OFF {
            self.stick = None;
        }
        // held, it repeats
        let held = self.dpad.or(self.stick);
        match (held, self.next) {
            (Some(direction), Some(next)) if held == held_before && Instant::now() >= next => {
                presses.push(direction);
                self.next = Some(next.max(Instant::now() - REPEAT) + REPEAT);
            }
            (None, _) => self.next = None,
            _ => {}
        }
        presses
    }
}

fn dpad(button: Button) -> Option<Press> {
    match button {
        Button::DPadUp => Some(Press::Up),
        Button::DPadDown => Some(Press::Down),
        Button::DPadLeft => Some(Press::Left),
        Button::DPadRight => Some(Press::Right),
        _ => None,
    }
}

/// the direction the stick points, keeping the one it had until it is let
/// go of enough. y grows downwards.
fn stick(had: Option<Press>, x: f32, y: f32) -> Option<Press> {
    if x.abs().max(y.abs()) < STICK_ON {
        // between the two, what it pointed at stays
        return had.filter(|_| x.abs().max(y.abs()) >= STICK_OFF);
    }
    Some(match (x.abs() > y.abs(), x > 0.0, y > 0.0) {
        (true, true, _) => Press::Right,
        (true, false, _) => Press::Left,
        (false, _, true) => Press::Down,
        (false, _, false) => Press::Up,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stick_points_with_some_give() {
        assert_eq!(stick(None, 0.0, 0.3), None);
        assert_eq!(stick(None, 0.0, 0.7), Some(Press::Down));
        assert_eq!(stick(None, -0.8, 0.2), Some(Press::Left));
        // easing off a little keeps it, letting go drops it
        assert_eq!(stick(Some(Press::Down), 0.0, 0.4), Some(Press::Down));
        assert_eq!(stick(Some(Press::Down), 0.0, 0.1), None);
    }
}
