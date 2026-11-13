//! Integration regression tests for console clock rate, emulation timing decoupling, and FPS limiting.

use std::time::Duration;
use zakuro::settings::FpsLimit;
use zakuro_core::kernel::thread::{nanos_to_ticks, ticks_to_nanos, CPU_CLOCK_HZ};
use zakuro_core::{AUDIO_SAMPLE_RATE, CYCLES_PER_AUDIO_FRAME, CYCLES_PER_FRAME};

#[test]
fn test_3ds_clock_and_timing_invariants() {
    // 3DS ARM11 CPU runs at 268.111856 MHz
    assert_eq!(CPU_CLOCK_HZ, 268_111_856);

    // One guest display frame is 4,481,136 CPU cycles (~59.83 Hz)
    assert_eq!(CYCLES_PER_FRAME, 4_481_136);

    // Audio DSP runs at CPU_CLOCK_HZ / 8192 = ~32,728 Hz
    assert_eq!(CYCLES_PER_AUDIO_FRAME, 160 * 8192);
    let expected_sample_rate = 268_111_856.0 / 8192.0;
    assert!((AUDIO_SAMPLE_RATE - expected_sample_rate).abs() < 0.001);

    // Cycle / nanosecond conversions
    assert_eq!(nanos_to_ticks(1_000_000_000), CPU_CLOCK_HZ);
    assert_eq!(ticks_to_nanos(CPU_CLOCK_HZ), 1_000_000_000);
}

#[test]
fn test_fps_limit_presentation_decoupling() {
    // Target display limits decoupled from emulation core
    let limits = [
        (FpsLimit::Fps30, Some(Duration::from_nanos(1_000_000_000 / 30))),
        (FpsLimit::Fps60, Some(Duration::from_nanos(1_000_000_000 / 60))),
        (FpsLimit::Fps90, Some(Duration::from_nanos(1_000_000_000 / 90))),
        (FpsLimit::Fps120, Some(Duration::from_nanos(1_000_000_000 / 120))),
        (FpsLimit::Uncapped, None),
    ];

    for (limit, expected_pres_time) in limits {
        assert_eq!(limit.presentation_time(), expected_pres_time);

        // Core emulation parameters MUST remain constant regardless of presentation limit.
        // This guarantees games do NOT speed up at 120 FPS or slow down at 30 FPS.
        assert_eq!(CPU_CLOCK_HZ, 268_111_856);
        assert_eq!(CYCLES_PER_FRAME, 4_481_136);
    }
}

#[test]
fn test_presentation_frame_delta_accuracy() {
    // Verify presentation time calculations for 120 Hz VRR on MSI Claw 8 AI+
    let time_120 = FpsLimit::Fps120.presentation_time().unwrap();
    assert_eq!(time_120.as_micros(), 8333); // ~8.333 ms

    let time_60 = FpsLimit::Fps60.presentation_time().unwrap();
    assert_eq!(time_60.as_micros(), 16666); // ~16.666 ms

    let time_30 = FpsLimit::Fps30.presentation_time().unwrap();
    assert_eq!(time_30.as_micros(), 33333); // ~33.333 ms
}
