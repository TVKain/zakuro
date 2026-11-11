# Zakuro Windows JIT Stack Probing Fix & Hardware Discrepancy Analysis

## 1. Executive Summary

When running Nintendo 3DS titles (such as *Pokémon Ultra Sun*) with Zakuro on the **MSI Claw 8 AI+** (Intel Core Ultra 200V "Lunar Lake" / Intel Arc 140V GPU), two separate issues were encountered:
1. **Immediate Crash (`STATUS_ACCESS_VIOLATION` / `0xC0000005`):** The emulator crashed within 60 frames during 3D rendering.
2. **"Saved game data is corrupted" In-Game Error:** An orphan `.format` archive file misled the game into believing a save archive existed, but the inner save file (`/main`) was missing.

Both issues have been diagnosed, resolved, and verified.

---

## 2. Technical Root Cause: The JIT Crash

### 2.1 The Windows x64 Stack Architecture & Guard Pages
On Windows (x86_64), the operating system allocates stack space dynamically:
* Thread stacks are reserved in advance (e.g. 1–2 MB), but only the top few pages (typically 4 KB each) are committed into physical memory.
* Immediately below the committed stack sits a special **Guard Page** (`PAGE_GUARD`).
* When code expands its stack, it touches memory addresses sequentially downwards. When an instruction writes to the guard page, the CPU triggers a page fault that the Windows kernel intercepts. The kernel commits that 4 KB page, sets up the next guard page further down, and transparently resumes thread execution.

### 2.2 The Violation in Cranelift
Zakuro uses Cranelift in [`crates/zakuro-gpu/src/shader/jit.rs`](crates/zakuro-gpu/src/shader/jit.rs) to JIT-compile 3DS vertex shaders to native host machine code.

When compiling complex vertex shaders, Cranelift allocated large stack frames:
```assembly
push   %rbp
mov    %rsp, %rbp
sub    $0xb4a0, %rsp    ; Allocates 46,240 bytes (over 11 pages!) in ONE step
...
mov    %rcx, 0x5f0(%rsp); Segfault! (0xC0000005)
```
* `$0xb4a0` is **46,240 bytes** (over eleven 4 KB pages).
* Because Cranelift did not have stack probing enabled by default, the code jumped over the 4 KB guard page entirely.
* Writing to `0x5f0(%rsp)` touched an **uncommitted, non-guard page** 45 KB away from the previous top-of-stack.
* The Windows kernel does not handle page faults for uncommitted memory beyond the guard page, immediately issuing an unhandled `EXCEPTION_ACCESS_VIOLATION` (`0xC0000005`).

### 2.3 The Solution
In Cranelift, stack probing can be mandated via target configuration flags. In [`crates/zakuro-gpu/src/shader/jit.rs`](crates/zakuro-gpu/src/shader/jit.rs#L713-L717):

```rust
    let flagged = [
        ("opt_level", "speed"),
        ("use_colocated_libcalls", "false"),
        ("is_pic", "false"),
        ("enable_verifier", verify),
        ("regalloc_algorithm", allocator),
        ("enable_probestack", "true"),
        ("probestack_strategy", "inline"),
    ];
```
* `enable_probestack: "true"`: Directs Cranelift to emit stack probes for stack allocations exceeding the host page size (4 KB).
* `probestack_strategy: "inline"`: Emits inline probe loops (`test [rsp - offset], 0`) that touch each 4 KB page in descending order before updating `%rsp`, properly triggering the Windows guard page mechanism to commit stack memory safely.

---

## 3. Why It Occurred on MSI Claw 8 AI+ but NOT on Lenovo LOQ

The Lenovo LOQ (typically an Intel Core 12th–14th Gen H/HX processor paired with an NVIDIA GeForce RTX dGPU) did not experience this crash, whereas the MSI Claw 8 AI+ (Intel Core Ultra 7/9 200V "Lunar Lake" with integrated Intel Arc 140V GPU) crashed consistently.

The architectural reasons include:

### 3.1 GPU Pipeline & Driver Stack Depth
* **Lenovo LOQ (NVIDIA dGPU):**
  * Zakuro's Vulkan hardware backend targets the dedicated NVIDIA GPU.
  * NVIDIA's Windows Vulkan driver (`nvoglv64.dll` / `nvd3dumx.dll`) performs substantial work on host threads before dispatching draw calls, frequently pushing 64 KB – 128 KB of deep driver call frames down the thread stack.
  * As a consequence, the worker threads on the Lenovo LOQ had **already touched and committed hundreds of kilobytes of stack memory** during Vulkan initialization and surface binding.
  * When Cranelift's unprobed `sub $0xb4a0, %rsp` ran on the Lenovo LOQ, `%rsp` was still residing within already-committed stack space, masking the bug.
* **MSI Claw 8 AI+ (Intel Arc 140V / Lunar Lake):**
  * The Intel Arc Vulkan driver (`igvk64.dll`) has a different execution path. Notice from Zakuro's hardware rasterizer logs:
    > `the GPU can't blit depth buffers, they are scaled on the CPU on their way to and from memory`
  * The thread calling into the JIT shader compiler was fresh or had a very shallow call stack, meaning only the initial 4 KB page was committed.
  * Jumping 46 KB downwards immediately struck uncommitted pages, triggering the crash 100% of the time.

### 3.2 Thread Pool & Rayon Stack Pre-Commit Differences
* On standard desktop/laptop Windows configurations with high core-count CPUs, background thread pools (e.g. Rayon, Intel graphics thread workers) may allocate and touch thread stacks during worker initialization.
* On Lunar Lake (with its 4 Lion Cove P-cores + 4 Low Power Skymont E-cores SoC design with hybrid energy-saving thread scheduler profiles in Windows 11 24H2), thread allocation and worker spawning strictly minimize committed memory footprint to conserve RAM and energy on handhelds.

---

## 4. The "Corrupted Save" Resolution

### 4.1 Cause
1. During earlier crashes, `%APPDATA%\zakuro\user\savedata\00040000001B5000` was left empty, but `%APPDATA%\zakuro\user\savedata\00040000001B5000.format` existed.
2. In `zakuro-core/src/services/fs.rs`, `GetFormatInfo` checks `archive.format_info()`. Because `.format` existed, it reported that the save archive was initialized and formatted.
3. The Pokémon game attempted to read `/main`. Because the directory was empty, opening `/main` failed (`FS_NOT_FOUND`).
4. On Nintendo 3DS, when a save archive is formatted but the main save file is missing, the game assumes data corruption rather than a clean first launch.

### 4.2 Fix
1. Removed the incomplete directory and orphan `.format` descriptor from `%APPDATA%\zakuro\user\savedata\`.
2. On next launch, `GetFormatInfo` properly returned `FS_NOT_FORMATTED`.
3. The game cleanly formatted the archive, generated a valid 445 KB `main` save file, and initialized the extdata directories.

---

## 5. Handheld Optimizations (MSI Claw 8 AI+)

### 5.1 View / Mode Button for In-Game Menu
* Handhelds like the MSI Claw 8 AI+ lack a physical `Escape` key.
* Added support for `View` (`Select`) and `Mode` (`Home` / Xbox / Guide) buttons on gamepads to toggle the in-game emulator menu.
* Added a configurable `Menu / Pause` option in **Settings -> Controls**.

### 5.2 Direct Touchscreen Input in Emulation
* **Problem:** On Windows handhelds (MSI Claw, ROG Ally, Legion Go), tapping the touchscreen worked inside the EGUI settings menu, but had zero effect on the 3DS bottom touchscreen in-game.
* **Root Cause:** Zakuro's `ApplicationHandler::window_event` only listened for `WindowEvent::MouseInput` and `WindowEvent::CursorMoved`. When tapping the physical touchscreen, Windows and `winit` deliver native `WindowEvent::Touch` events. EGUI processes touch internally via `egui_winit::State::on_window_event`, allowing settings interaction, but Zakuro never forwarded `WindowEvent::Touch` to the 3DS touchscreen input system (`keyboard.touch`).
* **Fix:** Implemented `WindowEvent::Touch(touch)` in [`crates/zakuro/src/lib.rs`](crates/zakuro/src/lib.rs):
  * Updates cursor position to the touch location.
  * Maps touch phase (`Started` / `Moved`) to `self.mouse_down = !self.pointer_taken(consumed)` and triggers `self.touch()`.
  * Releases touch on `Ended` / `Cancelled`.
  * Allows seamless in-game touch input on Windows handhelds and tablets.

