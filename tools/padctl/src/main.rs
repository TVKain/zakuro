//! plugs in a virtual Xbox 360 pad through ViGEmBus and follows commands on
//! stdin, one per line, answering "ok" after each:
//!   press A 90        press and release buttons (A+B for both), held 90 ms
//!   hold DOWN 700     hold buttons that long, then release
//!   stick 0 -1 500    hold the left stick (-1..1) that long, then centre
//!   trig L 1 90       pull a trigger fully, then release
//!   wait 300
//!   quit
//! after each release it waits GAP, as a hand would: Windows.Gaming.Input
//! reads the pad at intervals, and a release shorter than one is never seen.
use std::io::{BufRead, Write};
use std::{thread, time::Duration};
use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};

const GAP: Duration = Duration::from_millis(60);

fn button(name: &str) -> u16 {
    match name {
        "A" => XButtons::A, "B" => XButtons::B, "X" => XButtons::X, "Y" => XButtons::Y,
        "UP" => XButtons::UP, "DOWN" => XButtons::DOWN, "LEFT" => XButtons::LEFT, "RIGHT" => XButtons::RIGHT,
        "START" => XButtons::START, "BACK" => XButtons::BACK, "GUIDE" => XButtons::GUIDE,
        "LB" => XButtons::LB, "RB" => XButtons::RB, "LTHUMB" => XButtons::LTHUMB, "RTHUMB" => XButtons::RTHUMB,
        other => panic!("unknown button {other}"),
    }
}

fn main() {
    let client = Client::connect().expect("ViGEmBus not reachable");
    let mut pad = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
    pad.plugin().expect("plugin");
    pad.wait_ready().expect("ready");
    let neutral = XGamepad::default();
    pad.update(&neutral).unwrap();
    println!("ready");
    std::io::stdout().flush().unwrap();
    let ms = |s: &str| Duration::from_millis(s.parse().unwrap());
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let parts: Vec<&str> = line.trim_start_matches('\u{feff}').split_whitespace().collect();
        match parts.as_slice() {
            ["press" | "hold", buttons, time] => {
                let raw = buttons.split('+').map(button).fold(0, |a, b| a | b);
                pad.update(&XGamepad { buttons: XButtons { raw }, ..Default::default() }).unwrap();
                thread::sleep(ms(time));
                pad.update(&neutral).unwrap();
                thread::sleep(GAP);
            }
            ["stick", x, y, time] => {
                let axis = |v: &str| (v.parse::<f32>().unwrap() * 32767.0) as i16;
                pad.update(&XGamepad { thumb_lx: axis(x), thumb_ly: axis(y), ..Default::default() }).unwrap();
                thread::sleep(ms(time));
                pad.update(&neutral).unwrap();
                thread::sleep(GAP);
            }
            ["trig", side, v, time] => {
                let v = (v.parse::<f32>().unwrap() * 255.0) as u8;
                let g = if *side == "L" { XGamepad { left_trigger: v, ..Default::default() } } else { XGamepad { right_trigger: v, ..Default::default() } };
                pad.update(&g).unwrap();
                thread::sleep(ms(time));
                pad.update(&neutral).unwrap();
                thread::sleep(GAP);
            }
            ["wait", time] => thread::sleep(ms(time)),
            ["quit"] => break,
            [] => {}
            _ => eprintln!("bad command: {line}"),
        }
        println!("ok");
        std::io::stdout().flush().unwrap();
    }
    pad.unplug().ok();
}
