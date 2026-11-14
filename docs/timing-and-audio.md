# Timing and audio

## Clocks

| | Value | Where |
| :--- | :--- | :--- |
| ARM11 clock | 268,111,856 Hz | `kernel::thread::CPU_CLOCK_HZ` |
| Cycles per console frame | 4,481,136 (59.83 Hz, 16.71 ms) | `CYCLES_PER_FRAME`, `FRAME_TIME` |
| DSP output | 32,728 Hz | `AUDIO_SAMPLE_RATE` |

`timing_fps_regression.rs` pins these. The console's speed never depends on
the frame rate limit: 30, 60, 90, 120 and Uncapped change only how often a
picture is shown.

## The frame loop

`App::step` in `crates/zakuro/src/lib.rs` (see
[architecture.md](architecture.md#one-frame-from-game-code-to-the-screen)):

- **Emulate** when `next_frame` is due. After a frame, `next_frame +=
  FRAME_TIME`. If it falls more than `CATCH_UP_LIMIT` behind, it resets rather
  than racing to catch up.
- **Present at once** (`shows_now`). A newly emulated frame is presented in
  the same step. At 60 FPS or more every frame shows. At 30 FPS every other one
  does (a quarter-frame early is allowed). Above the game's rate, one repeat
  (or one interpolated picture) shows halfway between frames, never just
  before the next one. `next_shown` counts from the present.
- **Uncapped** presents at the display's refresh (`App::refresh`, from
  `timing::refresh()`, 240 Hz if unknown), not on every loop.
- **Sleep** until the next real event (`sleep_for`), with 1 ms timer
  resolution on Windows.
- **Fast forward** (Tab, or a free right trigger) runs frames back to back,
  shows at the console's rate, holds the audio, and skips pacing.

### Locked to the display (`pacing.rs`)

When the display is within 1% of a whole number of refreshes per console frame
(60, 59.94, 120, 240 Hz, but not 144 or 75), each frame's start is nudged by
at most 0.5 ms per frame, so it's ready 1.5 ms before a vblank. The estimate
used is the longest input → present time of the last 120 frames. The game
then runs at the display's rate (+0.29% at 60/120 Hz), and the audio speed
follows (`pacing::speed`), so the sound queue doesn't grow. Without the lock,
presenting at once cuts lag but leaves about 47% of frames off-beat.

## The audio path

1. **Voices** (`zakuro-core/src/services/dsp_voices.rs`). Each voice plays its
   buffers with the interpolation the game picked. Polyphase (mode 0, the
   default) plays through a 4-point Catmull-Rom cubic (`play_steady::<_,
   CUBIC>`). At buffer edges the missing neighbours repeat the edge sample.
   Linear (1) and None (2) are as named. Tests compare the block path against
   a sample-at-a-time reference, bit for bit.
2. **Mixer** (`dsp_mixer.rs`) to stereo at 32,728 Hz. The frontend takes it
   with `System::take_audio` after each frame.
3. **Queue and stretcher** (`crates/zakuro/src/audio.rs`). `TARGET` is 1280
   console samples (39 ms), with stretching below 640. The WSOLA stretcher
   (`Stretch`) passes sound straight through while idle, keeping only
   `HOP + 2·SEARCH` samples of history. It starts and stops stretching
   without a dip, and flushes back to idle when the speed returns to 1.0.
4. **Resampler** (`Kernel`). Converts 32.73 kHz to the device rate with a
   64-tap Kaiser-windowed sinc (β 9, cutoff 0.96 × the lower Nyquist), 256
   interpolated phases, each row normalised to 1. It adds about 1 ms. Test
   `tones_come_through_clean` holds it to 90–118 dB against exact sines.
5. **Output** through cpal on the default device.

Cushion sizes tried, with frames locked to the display: 4096 samples gave
134 ms of lag behind the picture, 1536 gave 52 ms, 1024 gave 39 ms but
dropouts during play, and 768 gave frequent dropouts and worse sync. 1280 is
the smallest size with no dropouts on the Claw.

## Measuring

`ZAKURO_TIMING=<file>` (`timing.rs`) writes, in QPC ticks:

| Line | Fields |
| :--- | :--- |
| `F <freq>` | Ticks per second |
| `E <started> <finished> <due> <frame>` | An emulated frame. `started` is when input was read |
| `P <started> <finished> <frame> <vblank> <refresh>` | A present, with the last vblank and refresh period from `DwmGetCompositionTimingInfo` |
| `A <added> <heard> <frame>` | A frame's sound. `heard` estimates when its first sample leaves the speakers: last callback + cpal's delay + buffer, plus what waits in the queue and stretcher |

`tools/timing_report.py <record> [first] [last]` counts a frame as shown at
the first vblank after its present. Windowed DWM adds one refresh on top of
that, the same for every variant, so comparisons hold. The report gives:

- **Input read → vblank**: the input lag Zakuro controls.
- **Shown interval in refreshes** and **off-beat**: the share of frames shown
  for more or fewer refreshes than usual.
- **Dropped**: frames emulated but never shown.
- **Sound after picture**: `heard` minus vblank. The true gap is about one
  refresh smaller, since DWM shows the picture a refresh later.

`tools/timing_run.ps1` runs a game in a sandbox with the overworld-walk replay
and prints the report ([testing.md](testing.md#measuring-timing)). Always use
recompiled code, the same settings, and the same machine for before and after.

## Current results

Ultra Sun overworld walk, recompiled, 4x, Sharp, on the MSI Claw 8 AI+ with
its 120 Hz panel ([devices.md](devices.md)). Input → vblank is the mean, with
p99 in brackets.

| Limit | Input → vblank | Off-beat | Sound after picture |
| :--- | :--- | :--- | :--- |
| 60 | 6.8 ms (7.4) | 0.35% | 44 ms (p99 52–57) |
| 120 | 6.6 ms (10.4) | 3.4% | 44 ms |
| Uncapped | 6.7 ms (9.7), 5.3k presents | 2.5% | |

Before this work: 60 FPS 10.9 ms (p99 20.6) with 11% off-beat, and sound
146 ms behind the picture. ITU-R BT.1359 puts noticeable trailing sound at
about 125 ms.
