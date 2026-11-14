# Rendering

How the PICA200's output becomes pixels in the window. The history behind
these choices is in [history.md](history.md).

## Drawing the 3D

`zakuro-gpu` receives command lists from the `gsp` service, decodes registers
(`registers.rs`) and turns draws into `DrawCall`s for a `Renderer`
(`renderer.rs`):

- **Hardware rasterizer** (`raster/hardware.rs`, default). Fills triangles
  through Vulkan, with the fragment stages (texture units, fragment lighting,
  six TEV combiners, alpha test) in `shaders/raster.frag`. Draws at an internal
  resolution of 1x–8x. `Hardware::set_scale` changes it in a running game: it
  waits for the device to go idle and rebuilds surfaces. Samplers use 16x
  anisotropic filtering above 1x.
- **Software rasterizer** (`raster.rs`). The reference. It follows the
  hardware, and the GPU shaders follow it. Use `--rasterizer software` to tell
  a hardware rasterizer bug from an emulation bug.

**Vertex shaders** run in one of three places:

1. On the GPU, in `shaders/shade.vert`, which interprets the title's shader
   program. Used when the device is a discrete GPU or its name contains Arc,
   Iris, GeForce or Radeon (the Arc 140V reports itself as integrated).
2. On the CPU through the Cranelift JIT (`shader/jit.rs`). Inline stack
   probing must stay on (`enable_probestack`, `probestack_strategy = inline`),
   or large shader frames crash on Windows ([history.md](history.md#cranelift-jit-stack-probing-crash)).
3. On the CPU through the interpreter (`shader/mod.rs`), with
   `ZAKURO_SHADER_JIT=0`.

## Textures

- **Decoding** of the PICA200 formats and tiling: `texture.rs`, `format.rs`.
- **Upscale textures** (setting): `shaders/scale.comp` redraws each texture 4x
  larger with alpha-aware xBRZ (2x at 2x/3x resolution, or for big textures),
  blended 35% with bilinear to round off hard diagonal steps, then blits the
  mip levels. Samplers shift their LOD by the extra levels
  (`Filtering::scaled`). Textures rewritten at the same address within 300
  batches (video, animations) skip upscaling. Toggling the setting rebuilds
  the texture cache.
- **Custom textures** (texture packs, `pack.rs`): Citra/Azahar packs, PNG only,
  loaded in the background. `System::set_texture_packs` turns them on and off
  in a running game.

## Frame interpolation

`interpolation.rs`: for games that draw every other refresh, each frame's
draws are matched with the previous frame's. Every draw that moved is drawn
again with its shader's float uniforms halfway between, into twin buffers,
which gives a picture between the two. Shown by the frame loop halfway between
frames ([timing-and-audio.md](timing-and-audio.md)).

## Presenting

`backend/` holds two presenters behind `present.rs`'s `Backend`:

- **Vulkan** (`backend/vulkan.rs`, `vulkan_overlay.rs`). When the renderer
  shares its device (`device.rs`), the screens are sampled straight from GPU
  memory (`Upright`). Otherwise they're uploaded. Present mode preference:
  MAILBOX, IMMEDIATE, FIFO_RELAXED, then FIFO (`ZAKURO_PRESENT_MODE` forces
  one). Handles `ERROR_SURFACE_LOST_KHR` and out-of-date swapchains.
- **OpenGL 3.3 core** (`backend/gl.rs`) through glutin, with swap interval 1.

The Graphics API setting switches between them on Home by recreating the
window and presenter (`App::switch_presenter`). The egui overlay is drawn in
the same pass.

### Linear light

Screens are sampled through sRGB views, so the texture unit decodes to linear
before any blending. Vulkan uses `R8G8B8A8_SRGB` screen images, and an extra
sRGB view for GPU-drawn upright images (`MUTABLE_FORMAT`, `Upright::linear`).
OpenGL uses `SRGB8_ALPHA8` textures. The shader encodes the output unless the
swapchain is already sRGB (push constant `srgb_window`, offset 52). Fragment
push constants are 40 bytes (80 for stereo). xBRZ makes its edge decisions on
encoded values, and the Dubois anaglyph works on encoded values.

### Display filters

Defined once in `shaders/present_common.glsl`, used by both presenters.
`present.frag` includes it, and `gl.rs` builds `HEADER + COMMON + WHOLE +
main`. The file must stay valid in GLSL 450 and GLSL 330 core.

| Setting (`settings::Filter`) | Shader mode (`ScreenFilter`) | Enlarging | Shrinking (internal resolution larger than the window) |
| :--- | :--- | :--- | :--- |
| **Sharp** (default) | 2 | Catmull-Rom, clamped to the 4 nearest texels, so it never rings | Catmull-Rom stretched over the footprint |
| Soft | 0 | Mitchell cubic (B = C = 1/3) | Mitchell stretched over the footprint |
| Pixelated | 1 | Sharp bilinear: square pixels without uneven widths | Exact area average |
| Smooth edges | 4 | xBRZ + linear | Catmull-Rom stretched over the footprint |

Mode 3 (plain xBRZ) is still in the shader but isn't offered. Old setting
names load as these: `smooth` → Soft, `pixels` → Pixelated, `xbrz` and
`xbrz_linear` → Smooth edges. The mode numbers are checked by
`resolution_presentation_regression.rs`.

When shrinking, each window pixel covers `s` texels, and the cubic's support
is stretched to 2·s. Neighbouring texels whose weights have the same sign
share one bilinear tap. The weights are separable, so this is exact. Taps are
generated in a loop rather than stored, since GPUs spill arrays to memory.
That leaves no tap limit, and the cost is about four taps per source texel at
any size. Up to 8 taps a row (shrinking up to about 3.5x), a row's taps are
computed once into a fixed-size array that the compiler keeps in registers
(`CACHED`).

Cost on the Arc 140V at a 1000×600 window: Sharp at 4x 265 µs, 6x 435 µs;
Pixelated 130 µs.

### How the kernels were chosen

The overworld frame was rendered at 8x and reduced to the window size with
Lanczos3 in linear light (and separately with an exact area average). That is
the reference. Renders at 1x–6x through each filter were scored against it
with PSNR, SSIM and an edge-energy ratio (`tools/imgbench`, see
[testing.md](testing.md#measuring-picture-quality)). Sharp at 4x into
1000×600 went from 35.85 dB (the old CAS) to 37.15 dB, and 5x reached
39.18 dB. Linear light alone is worth 0.5–0.9 dB. When enlarging, all filters
land within about 0.3 dB, so there the choice is taste. Use the same method
for any new filter.

## Shaders

The program embeds the committed SPIR-V. After editing a shader, rebuild its
`.spv` from `crates/zakuro-gpu/shaders`:

| Output | Command |
| :--- | :--- |
| `present.frag.spv` | `glslangValidator -V present.frag -o present.frag.spv` |
| `present_stereo.frag.spv` | `glslangValidator -V -DSTEREO present.frag -o present_stereo.frag.spv` |
| `raster_depth.frag.spv` | `glslangValidator -V -DWRITES_DEPTH raster.frag -o raster_depth.frag.spv` |
| `upright_image.comp.spv` | `glslangValidator -V -DTO_IMAGE upright.comp -o upright_image.comp.spv` |
| every other `<name>.spv` | `glslangValidator -V <name> -o <name>.spv` |

Editing `present_common.glsl` means rebuilding both present outputs. The
OpenGL presenter picks it up at build time.

Most of the upstream `.spv` files were built with `glslc` (shaderc), so
rebuilding one with `glslangValidator` changes its bytes and size but not
its behaviour. Rebuild only the shaders you changed, so diffs stay readable.
`tools/test_regressions.ps1` step 4 compiles every shader and variant into a
temporary folder. When you add a shader or variant, add it to that list too.
