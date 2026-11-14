# Test devices

Measurements in these docs only mean something next to the machine they were
taken on. When you test or measure on a machine not listed here, add it, and
say in [history.md](history.md) or the topic doc which device a number came
from.

## Identifying the machine

Run these at the start of a session ([AGENTS.md](../AGENTS.md#at-the-start-of-every-session-onboard-the-device)).

Windows (PowerShell):

```powershell
Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer, Model, TotalPhysicalMemory
Get-CimInstance Win32_Processor | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors
Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion, CurrentHorizontalResolution, CurrentVerticalResolution, CurrentRefreshRate
[Environment]::OSVersion.Version
```

Linux:

```bash
cat /sys/devices/virtual/dmi/id/product_name; lscpu | head -20; uname -r
vulkaninfo --summary 2>/dev/null | grep -E 'deviceName|driverVersion'; xrandr --current 2>/dev/null | grep '\*'
```

macOS:

```bash
sysctl -n machdep.cpu.brand_string hw.model; sw_vers; system_profiler SPDisplaysDataType
```

Notes:

- A virtual display adapter (Parsec, remote desktop) can be listed next to the
  real GPU. The refresh rate that matters is that of the display the window is
  on. `ZAKURO_TIMING` records the period DWM actually reports, so check it there.
- A handheld's panel can run at a lower rate than its maximum (power saving,
  or a setting). Note the rate in effect for each measurement, not the panel's
  maximum.

## Devices

### MSI Claw 8 AI+ (A2VM): primary

| | |
| :--- | :--- |
| CPU | Intel Core Ultra 7 258V, "Lunar Lake": 4 Lion Cove P-cores and 4 Skymont low-power E-cores |
| GPU | Intel Arc 140V (Xe2, 8 cores, integrated, 16 GB shared). Vulkan 1.3 and OpenGL 4.6 |
| Display | Built-in 8" 1920×1200, 120 Hz with VRR. Also has a Parsec virtual display adapter installed |
| OS | Windows 11 (build 26200) |
| Input | Built-in Xbox-style controller (XInput), touchscreen. No Escape key, so the Menu/Home button opens the pause menu |

Used for all handheld UI work, every timing and audio measurement, the
presenter benchmarks, and the full Ultra Sun recompile timings. Notable
findings:

- **JIT stack crash.** Cranelift's large stack frames skipped the guard page,
  because the Intel driver commits little thread stack. Fixed with inline stack
  probing ([history.md](history.md#cranelift-jit-stack-probing-crash)).
- **The Arc 140V reports itself as an integrated GPU.** The old rule ran vertex
  shaders on the CPU for integrated GPUs, and Arc, Iris, GeForce and Radeon now
  shade on the GPU. That takes about 48 µs per frame on the Arc.
- **Presenter cost** at 1000×600: Sharp at 4x 265 µs, 6x 435 µs; Pixelated
  130 µs ([rendering.md](rendering.md)).
- **Frame pacing at 120 Hz.** 60 FPS limit: input → vblank 6.8 ms (p99 7.4),
  0.35% off-beat. Sound 44 ms after the picture
  ([timing-and-audio.md](timing-and-audio.md)).
- **Recompile** of Ultra Sun with 7 jobs: 726 s with `-g0`, 1055 s without.

### Lenovo LOQ (NVIDIA GeForce RTX): secondary

| | |
| :--- | :--- |
| GPU | NVIDIA GeForce RTX (discrete) |
| OS | Windows 11 |

Used to compare against the Claw. The JIT stack crash never happened here,
because NVIDIA's driver commits 64–128 KB of thread stack during Vulkan
setup, so the oversized frames landed in committed memory. Bugs that appear
only on Intel or only on NVIDIA should be checked on both.

### CI runners

GitHub Actions `windows-latest`, `ubuntu-latest` and `macos-latest` build and
run the test suite for every push. They have no GPU and never run a game.
The macOS build hasn't been tried on a real Mac yet.

## Adding a device

When onboarding finds a machine not listed above, add a section with:

- CPU, GPU (and driver version, if a result depends on it), display (size,
  resolution, refresh rate, VRR), OS build, input devices.
- What you tested or measured on it, with the numbers and the settings
  (resolution, filter, frame rate limit, recompiled or not).
- Anything that behaved differently from the other devices.
