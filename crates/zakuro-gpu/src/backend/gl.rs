//! OpenGL presentation backend.

use std::collections::HashMap;

use glow::HasContext;

use super::{place, Overlay, OverlayVertex, PresentError, Presenter, ScreenFilter, ScreenImage, ScreenLayout, ScreenOrientation, Shown, Stereo, StereoMode, Viewport};

const VERTEX_SHADER: &str = r#"#version 330 core
// a single oversized triangle covers the viewport with no vertex buffer.
out vec2 uv;
uniform int orientation;
void main() {
    vec2 position = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
    uv = vec2(position.x, 1.0 - position.y);
    if (orientation == 90) {
        uv = vec2(uv.y, 1.0 - uv.x);
    } else if (orientation == 180) {
        uv = vec2(1.0 - uv.x, 1.0 - uv.y);
    } else if (orientation == 270) {
        uv = vec2(1.0 - uv.y, uv.x);
    }
    gl_Position = vec4(position * 2.0 - 1.0, 0.0, 1.0);
}
"#;

// a screen bigger than the window shows it is averaged over every texel a
// pixel covers, the way the Vulkan presenter does it. shown samples one of
// the pictures, the shaders put together from it draw one screen, or the
// top screen's two pictures in one for 3D.
const SAMPLE: &str = r#"#version 330 core
in vec2 uv;
out vec4 color;
uniform sampler2D screen;
// how a screen drawn bigger than its pixels is filtered: 0 smooth, 1
// pixels, 2 sharp, 3 xbrz, as in the Vulkan presenter's shader.
uniform int mode;

#define BLEND_NONE 0
#define BLEND_NORMAL 1
#define BLEND_DOMINANT 2
#define LUMINANCE_WEIGHT 1.0
#define EQUAL_COLOR_TOLERANCE (30.0 / 255.0)
#define STEEP_DIRECTION_THRESHOLD 2.2
#define DOMINANT_DIRECTION_THRESHOLD 3.6

float dist_ycbcr(vec3 a, vec3 b) {
    const vec3 w = vec3(0.2627, 0.6780, 0.0593);
    const float scale_b = 0.5 / (1.0 - w.b);
    const float scale_r = 0.5 / (1.0 - w.r);
    vec3 diff = a - b;
    float y = dot(diff, w);
    float cb = scale_b * (diff.b - y);
    float cr = scale_r * (diff.r - y);
    return sqrt(((LUMINANCE_WEIGHT * y) * (LUMINANCE_WEIGHT * y)) + (cb * cb) + (cr * cr));
}

bool is_pix_equal(vec3 a, vec3 b) {
    return dist_ycbcr(a, b) < EQUAL_COLOR_TOLERANCE;
}

float get_left_ratio(vec2 center, vec2 origin, vec2 direction, vec2 scale) {
    vec2 p0 = center - origin;
    vec2 proj = direction * (dot(p0, direction) / dot(direction, direction));
    vec2 distv = p0 - proj;
    vec2 orth = vec2(-direction.y, direction.x);
    float side = sign(dot(p0, orth));
    float v = side * length(distv * scale);
    return smoothstep(-0.70710678, 0.70710678, v);
}

bool eq(vec3 a, vec3 b) { return all(equal(a, b)); }
bool neq(vec3 a, vec3 b) { return any(notEqual(a, b)); }

vec3 tap(sampler2D img, vec2 base, vec2 offset, vec2 size) {
    return texture(img, clamp(base + offset / size, vec2(0.0), vec2(1.0))).rgb;
}

vec3 xbrz_sample(sampler2D img, vec2 at, vec2 size, vec2 covered) {
    vec2 scale = 1.0 / max(covered, vec2(1e-6));
    vec2 texel = at * size;
    vec2 pos = fract(texel) - vec2(0.5, 0.5);
    vec2 coord = (floor(texel) + 0.5) / size;

    vec3 A = tap(img, coord, vec2(-1.0, -1.0), size);
    vec3 B = tap(img, coord, vec2( 0.0, -1.0), size);
    vec3 C = tap(img, coord, vec2( 1.0, -1.0), size);
    vec3 D = tap(img, coord, vec2(-1.0,  0.0), size);
    vec3 E = tap(img, coord, vec2( 0.0,  0.0), size);
    vec3 F = tap(img, coord, vec2( 1.0,  0.0), size);
    vec3 G = tap(img, coord, vec2(-1.0,  1.0), size);
    vec3 H = tap(img, coord, vec2( 0.0,  1.0), size);
    vec3 I = tap(img, coord, vec2( 1.0,  1.0), size);

    ivec4 blend = ivec4(BLEND_NONE);

    // Corner bottom-right
    if (!((eq(E, F) && eq(H, I)) || (eq(E, H) && eq(F, I)))) {
        float dist_H_F = dist_ycbcr(G, E) + dist_ycbcr(E, C) + dist_ycbcr(tap(img, coord, vec2(0.0, 2.0), size), I) + dist_ycbcr(I, tap(img, coord, vec2(2.0, 0.0), size)) + 4.0 * dist_ycbcr(H, F);
        float dist_E_I = dist_ycbcr(D, H) + dist_ycbcr(H, tap(img, coord, vec2(1.0, 2.0), size)) + dist_ycbcr(B, F) + dist_ycbcr(F, tap(img, coord, vec2(2.0, 1.0), size)) + 4.0 * dist_ycbcr(E, I);
        bool dominant = (DOMINANT_DIRECTION_THRESHOLD * dist_H_F) < dist_E_I;
        blend.z = ((dist_H_F < dist_E_I) && neq(E, F) && neq(E, H)) ? ((dominant) ? BLEND_DOMINANT : BLEND_NORMAL) : BLEND_NONE;
    }

    // Corner bottom-left
    if (!((eq(D, E) && eq(G, H)) || (eq(D, G) && eq(E, H)))) {
        float dist_G_E = dist_ycbcr(tap(img, coord, vec2(-2.0, 1.0), size), D) + dist_ycbcr(D, B) + dist_ycbcr(tap(img, coord, vec2(-1.0, 2.0), size), H) + dist_ycbcr(H, F) + 4.0 * dist_ycbcr(G, E);
        float dist_D_H = dist_ycbcr(tap(img, coord, vec2(-2.0, 0.0), size), G) + dist_ycbcr(G, tap(img, coord, vec2(0.0, 2.0), size)) + dist_ycbcr(A, E) + dist_ycbcr(E, I) + 4.0 * dist_ycbcr(D, H);
        bool dominant = (DOMINANT_DIRECTION_THRESHOLD * dist_D_H) < dist_G_E;
        blend.w = ((dist_G_E > dist_D_H) && neq(E, D) && neq(E, H)) ? ((dominant) ? BLEND_DOMINANT : BLEND_NORMAL) : BLEND_NONE;
    }

    // Corner top-right
    if (!((eq(B, C) && eq(E, F)) || (eq(B, E) && eq(C, F)))) {
        float dist_E_C = dist_ycbcr(D, B) + dist_ycbcr(B, tap(img, coord, vec2(1.0, -2.0), size)) + dist_ycbcr(H, F) + dist_ycbcr(F, tap(img, coord, vec2(2.0, -1.0), size)) + 4.0 * dist_ycbcr(E, C);
        float dist_B_F = dist_ycbcr(A, E) + dist_ycbcr(E, I) + dist_ycbcr(tap(img, coord, vec2(0.0, -2.0), size), C) + dist_ycbcr(C, tap(img, coord, vec2(2.0, 0.0), size)) + 4.0 * dist_ycbcr(B, F);
        bool dominant = (DOMINANT_DIRECTION_THRESHOLD * dist_B_F) < dist_E_C;
        blend.y = ((dist_E_C > dist_B_F) && neq(E, B) && neq(E, F)) ? ((dominant) ? BLEND_DOMINANT : BLEND_NORMAL) : BLEND_NONE;
    }

    // Corner top-left
    if (!((eq(A, B) && eq(D, E)) || (eq(A, D) && eq(B, E)))) {
        float dist_D_B = dist_ycbcr(tap(img, coord, vec2(-2.0, 0.0), size), A) + dist_ycbcr(A, tap(img, coord, vec2(0.0, -2.0), size)) + dist_ycbcr(G, E) + dist_ycbcr(E, C) + 4.0 * dist_ycbcr(D, B);
        float dist_A_E = dist_ycbcr(tap(img, coord, vec2(-2.0, -1.0), size), D) + dist_ycbcr(D, H) + dist_ycbcr(tap(img, coord, vec2(-1.0, -2.0), size), B) + dist_ycbcr(B, F) + 4.0 * dist_ycbcr(A, E);
        bool dominant = (DOMINANT_DIRECTION_THRESHOLD * dist_D_B) < dist_A_E;
        blend.x = ((dist_D_B < dist_A_E) && neq(E, D) && neq(E, B)) ? ((dominant) ? BLEND_DOMINANT : BLEND_NORMAL) : BLEND_NONE;
    }

    vec3 res = E;

    // Blend bottom-right
    if (blend.z != BLEND_NONE) {
        float dist_F_G = dist_ycbcr(F, G);
        float dist_H_C = dist_ycbcr(H, C);
        bool doLineBlend = (blend.z == BLEND_DOMINANT ||
            !((blend.y != BLEND_NONE && !is_pix_equal(E, G)) || (blend.w != BLEND_NONE && !is_pix_equal(E, C)) ||
              (is_pix_equal(G, H) && is_pix_equal(H, I) && is_pix_equal(I, F) && is_pix_equal(F, C) && !is_pix_equal(E, I))));
        vec2 origin = vec2(0.0, 0.70710678);
        vec2 direction = vec2(1.0, -1.0);
        if (doLineBlend) {
            bool haveShallow = (STEEP_DIRECTION_THRESHOLD * dist_F_G <= dist_H_C) && neq(E, G) && neq(D, G);
            bool haveSteep = (STEEP_DIRECTION_THRESHOLD * dist_H_C <= dist_F_G) && neq(E, C) && neq(B, C);
            origin = haveShallow ? vec2(0.0, 0.25) : vec2(0.0, 0.5);
            direction.x += haveShallow ? 1.0 : 0.0;
            direction.y -= haveSteep ? 1.0 : 0.0;
        }
        vec3 blendPix = mix(H, F, step(dist_ycbcr(E, F), dist_ycbcr(E, H)));
        res = mix(res, blendPix, get_left_ratio(pos, origin, direction, scale));
    }

    // Blend bottom-left
    if (blend.w != BLEND_NONE) {
        float dist_H_A = dist_ycbcr(H, A);
        float dist_D_I = dist_ycbcr(D, I);
        bool doLineBlend = (blend.w == BLEND_DOMINANT ||
            !((blend.z != BLEND_NONE && !is_pix_equal(E, A)) || (blend.x != BLEND_NONE && !is_pix_equal(E, I)) ||
              (is_pix_equal(A, D) && is_pix_equal(D, G) && is_pix_equal(G, H) && is_pix_equal(H, I) && !is_pix_equal(E, G))));
        vec2 origin = vec2(-0.70710678, 0.0);
        vec2 direction = vec2(1.0, 1.0);
        if (doLineBlend) {
            bool haveShallow = (STEEP_DIRECTION_THRESHOLD * dist_H_A <= dist_D_I) && neq(E, A) && neq(B, A);
            bool haveSteep = (STEEP_DIRECTION_THRESHOLD * dist_D_I <= dist_H_A) && neq(E, I) && neq(F, I);
            origin = haveShallow ? vec2(-0.25, 0.0) : vec2(-0.5, 0.0);
            direction.y += haveShallow ? 1.0 : 0.0;
            direction.x += haveSteep ? 1.0 : 0.0;
        }
        vec3 blendPix = mix(H, D, step(dist_ycbcr(E, D), dist_ycbcr(E, H)));
        res = mix(res, blendPix, get_left_ratio(pos, origin, direction, scale));
    }

    // Blend top-right
    if (blend.y != BLEND_NONE) {
        float dist_B_I = dist_ycbcr(B, I);
        float dist_F_A = dist_ycbcr(F, A);
        bool doLineBlend = (blend.y == BLEND_DOMINANT ||
            !((blend.x != BLEND_NONE && !is_pix_equal(E, I)) || (blend.z != BLEND_NONE && !is_pix_equal(E, A)) ||
              (is_pix_equal(I, F) && is_pix_equal(F, C) && is_pix_equal(C, B) && is_pix_equal(B, A) && !is_pix_equal(E, C))));
        vec2 origin = vec2(0.70710678, 0.0);
        vec2 direction = vec2(-1.0, -1.0);
        if (doLineBlend) {
            bool haveShallow = (STEEP_DIRECTION_THRESHOLD * dist_B_I <= dist_F_A) && neq(E, I) && neq(H, I);
            bool haveSteep = (STEEP_DIRECTION_THRESHOLD * dist_F_A <= dist_B_I) && neq(E, A) && neq(D, A);
            origin = haveShallow ? vec2(0.25, 0.0) : vec2(0.5, 0.0);
            direction.y -= haveShallow ? 1.0 : 0.0;
            direction.x += haveSteep ? 1.0 : 0.0;
        }
        vec3 blendPix = mix(F, B, step(dist_ycbcr(E, B), dist_ycbcr(E, F)));
        res = mix(res, blendPix, get_left_ratio(pos, origin, direction, scale));
    }

    // Blend top-left
    if (blend.x != BLEND_NONE) {
        float dist_D_C = dist_ycbcr(D, C);
        float dist_B_G = dist_ycbcr(B, G);
        bool doLineBlend = (blend.x == BLEND_DOMINANT ||
            !((blend.w != BLEND_NONE && !is_pix_equal(E, C)) || (blend.y != BLEND_NONE && !is_pix_equal(E, G)) ||
              (is_pix_equal(C, B) && is_pix_equal(B, A) && is_pix_equal(A, D) && is_pix_equal(D, G) && !is_pix_equal(E, A))));
        vec2 origin = vec2(0.0, -0.70710678);
        vec2 direction = vec2(-1.0, 1.0);
        if (doLineBlend) {
            bool haveShallow = (STEEP_DIRECTION_THRESHOLD * dist_D_C <= dist_B_G) && neq(E, C) && neq(F, C);
            bool haveSteep = (STEEP_DIRECTION_THRESHOLD * dist_B_G <= dist_D_C) && neq(E, G) && neq(H, G);
            origin = haveShallow ? vec2(0.0, -0.25) : vec2(0.0, -0.5);
            direction.y -= haveShallow ? 1.0 : 0.0;
            direction.x += haveSteep ? 1.0 : 0.0;
        }
        vec3 blendPix = mix(D, B, step(dist_ycbcr(E, B), dist_ycbcr(E, D)));
        res = mix(res, blendPix, get_left_ratio(pos, origin, direction, scale));
    }

    return res;
}

vec3 shown(sampler2D image) {
    vec2 size = vec2(textureSize(image, 0));
    vec2 covered = fwidth(uv) * size;
    if (max(covered.x, covered.y) <= 1.001) {
        vec2 texel = uv * size;
        vec2 at = uv;
        if (mode == 1) {
            at = (floor(texel) + 0.5) / size;
        } else if (mode == 2) {
            vec2 scale = 1.0 / max(covered, vec2(1e-6));
            vec2 centred = texel - 0.5;
            vec2 blend = clamp((fract(centred) - 0.5) * scale + 0.5, 0.0, 1.0);
            at = (floor(centred) + 0.5 + blend) / size;
        } else if (mode == 3) {
            return xbrz_sample(image, uv, size, covered);
        }
        return texture(image, at).rgb;
    }

    if (mode == 1) {
        vec2 texel = (floor(uv * size) + 0.5) / size;
        return texture(image, texel).rgb;
    }

    return texture(image, uv).rgb;
}
"#;

const SCREEN_MAIN: &str = r#"
void main() {
    color = vec4(shown(screen), 1.0);
}
"#;

// as the Vulkan presenter's shader does it, with rows counted from the top
// of the window, which OpenGL counts from the bottom.
const STEREO_MAIN: &str = r#"
uniform sampler2D right_eye;
// 1 red and cyan, 2 alternate rows, whether the eyes swap, and the
// window's height
uniform int stereo;
uniform int swap;
uniform float height;
void main() {
    vec3 left = shown(screen);
    vec3 right = shown(right_eye);
    if (swap != 0) {
        vec3 other = left;
        left = right;
        right = other;
    }
    if (stereo == 1) {
        vec3 red = vec3(dot(left, vec3(0.437, 0.449, 0.164)), dot(left, vec3(-0.062, -0.062, -0.024)), dot(left, vec3(-0.048, -0.050, -0.017)));
        vec3 cyan = vec3(dot(right, vec3(-0.011, -0.032, -0.007)), dot(right, vec3(0.377, 0.761, 0.009)), dot(right, vec3(-0.026, -0.093, 1.234)));
        color = vec4(clamp(red + cyan, 0.0, 1.0), 1.0);
    } else {
        color = vec4((int(height - gl_FragCoord.y) & 1) == 0 ? left : right, 1.0);
    }
}
"#;

const OVERLAY_VERTEX_SHADER: &str = r#"#version 330 core
// the overlay's triangles, given in window pixels from the top left.
uniform vec2 size;
layout(location = 0) in vec2 position;
layout(location = 1) in vec2 uv;
layout(location = 2) in vec4 color;
out vec2 frag_uv;
out vec4 frag_color;
void main() {
    frag_uv = uv;
    frag_color = color;
    gl_Position = vec4(position.x / size.x * 2.0 - 1.0, 1.0 - position.y / size.y * 2.0, 0.0, 1.0);
}
"#;

const OVERLAY_FRAGMENT_SHADER: &str = r#"#version 330 core
in vec2 frag_uv;
in vec4 frag_color;
out vec4 color;
uniform sampler2D image;
void main() {
    color = frag_color * texture(image, frag_uv);
}
"#;

/// what draws the overlay.
struct GlOverlay {
    program: glow::Program,
    vertex_array: glow::VertexArray,
    vertices: glow::Buffer,
    indices: glow::Buffer,
    textures: HashMap<u64, glow::Texture>,
}

pub struct GlPresenter {
    gl: glow::Context,
    program: glow::Program,
    /// what draws the top screen's two pictures in one, for 3D.
    stereo_program: glow::Program,
    vertex_array: glow::VertexArray,
    overlay: GlOverlay,
    /// index 0 is the top screen, 1 the bottom, 2 the top one's picture
    /// for the right eye.
    textures: [glow::Texture; 3],
    /// dimensions each texture was last allocated at, so uploads can use
    /// tex_sub_image_2d when nothing changed.
    sizes: [(u32, u32); 3],
    window: (u32, u32),
    arrangement: ScreenLayout,
    orientation: ScreenOrientation,
    top_width: f32,
    filter: ScreenFilter,
    integer: bool,
    stereo: Stereo,
}

impl GlPresenter {
    /// loader resolves OpenGL function names, as the windowing library
    /// provides.
    ///
    /// # Safety
    ///
    /// A current OpenGL 3.3 context must be bound on this thread.
    pub unsafe fn new(
        loader: impl FnMut(&str) -> *const std::ffi::c_void,
        window: (u32, u32),
    ) -> Result<GlPresenter, PresentError> {
        let gl = unsafe { glow::Context::from_loader_function(loader) };

        let version = unsafe { gl.get_parameter_string(glow::VERSION) };
        let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
        log::info!("OpenGL {version} on {renderer}");

        let program = unsafe { link_program(&gl, VERTEX_SHADER, &format!("{SAMPLE}{SCREEN_MAIN}"))? };
        let stereo_program = unsafe { link_program(&gl, VERTEX_SHADER, &format!("{SAMPLE}{STEREO_MAIN}"))? };
        let vertex_array = unsafe { gl.create_vertex_array() }
            .map_err(PresentError::Backend)?;

        let mut textures = Vec::with_capacity(3);
        for _ in 0..3 {
            let texture = unsafe { gl.create_texture() }.map_err(PresentError::Backend)?;
            unsafe {
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                // linear, the screens come drawn at up to four times the
                // console's resolution and get shrunk as often as grown
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );
            }
            textures.push(texture);
        }

        let overlay = unsafe { GlOverlay::new(&gl)? };
        Ok(GlPresenter {
            gl,
            program,
            stereo_program,
            vertex_array,
            overlay,
            textures: [textures[0], textures[1], textures[2]],
            sizes: [(0, 0); 3],
            window,
            arrangement: ScreenLayout::default(),
            orientation: ScreenOrientation::default(),
            top_width: super::TOP_WIDTH,
            filter: ScreenFilter::default(),
            integer: false,
            stereo: Stereo::default(),
        })
    }

    fn upload(&mut self, index: usize, image: &ScreenImage<'_>) {
        let gl = &self.gl;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.textures[index]));
            if self.sizes[index] != (image.width, image.height) {
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    image.width as i32,
                    image.height as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(image.pixels)),
                );
                self.sizes[index] = (image.width, image.height);
            } else {
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    image.width as i32,
                    image.height as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(image.pixels)),
                );
            }
        }
    }

    /// the program, and the uniforms the two programs share.
    fn use_program(&self, program: glow::Program) {
        let gl = &self.gl;
        unsafe {
            gl.use_program(Some(program));
            if let Some(location) = gl.get_uniform_location(program, "screen") {
                gl.uniform_1_i32(Some(&location), 0);
            }
            if let Some(location) = gl.get_uniform_location(program, "mode") {
                gl.uniform_1_i32(Some(&location), self.filter.mode());
            }
            if let Some(location) = gl.get_uniform_location(program, "orientation") {
                gl.uniform_1_i32(Some(&location), self.orientation.clockwise_angle());
            }
        }
    }

    fn set_viewport(&self, viewport: Viewport) {
        // OpenGL's origin is the bottom left, so the y coordinate flips.
        let y = self.window.1 as f32 - viewport.y - viewport.height;
        unsafe { self.gl.viewport(viewport.x as i32, y as i32, viewport.width as i32, viewport.height as i32) };
    }

    fn draw_screen(&self, index: usize, viewport: Viewport) {
        let gl = &self.gl;
        self.use_program(self.program);
        self.set_viewport(viewport);
        unsafe {
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.textures[index]));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }
    }

    /// the top screen's two pictures in one, as the stereo mode mixes them.
    fn draw_both(&self, viewport: Viewport) {
        let gl = &self.gl;
        let program = self.stereo_program;
        self.use_program(program);
        self.set_viewport(viewport);
        unsafe {
            let set = |name: &str, value: i32| {
                if let Some(location) = gl.get_uniform_location(program, name) {
                    gl.uniform_1_i32(Some(&location), value);
                }
            };
            set("right_eye", 1);
            set("stereo", if self.stereo.mode == StereoMode::Anaglyph { 1 } else { 2 });
            set("swap", self.stereo.swap as i32);
            if let Some(location) = gl.get_uniform_location(program, "height") {
                gl.uniform_1_f32(Some(&location), self.window.1 as f32);
            }
            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.textures[2]));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.textures[0]));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }
    }
}

impl Presenter for GlPresenter {
    fn name(&self) -> &'static str {
        "opengl"
    }

    fn present(
        &mut self,
        top: ScreenImage<'_>,
        bottom: ScreenImage<'_>,
        right: ScreenImage<'_>,
        overlay: &Overlay,
    ) -> Result<(), PresentError> {
        if !top.is_empty() {
            self.upload(0, &top);
        }
        if !bottom.is_empty() {
            self.upload(1, &bottom);
        }
        let has_right = !right.is_empty();
        if has_right {
            self.upload(2, &right);
        }

        let gl = &self.gl;
        unsafe {
            gl.viewport(0, 0, self.window.0 as i32, self.window.1 as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.bind_vertex_array(Some(self.vertex_array));
        }

        for (viewport, shown) in place(self.window.0, self.window.1, self.arrangement, self.orientation, self.top_width, self.integer, self.stereo, has_right) {
            match shown {
                Shown::Top if !top.is_empty() => self.draw_screen(0, viewport),
                Shown::Bottom if !bottom.is_empty() => self.draw_screen(1, viewport),
                Shown::Right => self.draw_screen(2, viewport),
                Shown::Both if !top.is_empty() => self.draw_both(viewport),
                _ => {}
            }
        }

        unsafe {
            self.gl.bind_vertex_array(None);
            self.overlay.draw(&self.gl, overlay, self.window);
        }
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.window = (width.max(1), height.max(1));
    }

    fn set_layout(&mut self, arrangement: ScreenLayout) {
        self.arrangement = arrangement;
    }

    fn set_orientation(&mut self, orientation: ScreenOrientation) {
        self.orientation = orientation;
    }

    fn set_top_width(&mut self, width: f32) {
        self.top_width = width;
    }

    fn set_scaling(&mut self, filter: ScreenFilter, integer: bool) {
        self.filter = filter;
        self.integer = integer;
    }

    fn set_stereo(&mut self, stereo: Stereo) {
        self.stereo = stereo;
    }
}

impl Drop for GlPresenter {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_program(self.program);
            self.gl.delete_program(self.stereo_program);
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_program(self.overlay.program);
            self.gl.delete_vertex_array(self.overlay.vertex_array);
            self.gl.delete_buffer(self.overlay.vertices);
            self.gl.delete_buffer(self.overlay.indices);
            for (_, texture) in self.overlay.textures.drain() {
                self.gl.delete_texture(texture);
            }
            for texture in self.textures {
                self.gl.delete_texture(texture);
            }
        }
    }
}

impl GlOverlay {
    /// # Safety
    ///
    /// the context has to be current.
    unsafe fn new(gl: &glow::Context) -> Result<GlOverlay, PresentError> {
        unsafe {
            let program = link_program(gl, OVERLAY_VERTEX_SHADER, OVERLAY_FRAGMENT_SHADER)?;
            let vertex_array = gl.create_vertex_array().map_err(PresentError::Backend)?;
            let vertices = gl.create_buffer().map_err(PresentError::Backend)?;
            let indices = gl.create_buffer().map_err(PresentError::Backend)?;
            gl.bind_vertex_array(Some(vertex_array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vertices));
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(indices));
            let stride = size_of::<OverlayVertex>() as i32;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, 8);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 4, glow::UNSIGNED_BYTE, true, stride, 16);
            gl.bind_vertex_array(None);
            Ok(GlOverlay { program, vertex_array, vertices, indices, textures: HashMap::new() })
        }
    }

    /// # Safety
    ///
    /// the context has to be current.
    unsafe fn draw(&mut self, gl: &glow::Context, overlay: &Overlay, window: (u32, u32)) {
        unsafe {
            for texture in &overlay.textures {
                let [width, height] = texture.size;
                if texture.pixels.len() < (width * height * 4) as usize {
                    continue;
                }
                let pixels = glow::PixelUnpackData::Slice(Some(&texture.pixels));
                match texture.offset {
                    None => {
                        let Ok(made) = gl.create_texture() else { continue };
                        if let Some(old) = self.textures.insert(texture.id, made) {
                            gl.delete_texture(old);
                        }
                        gl.bind_texture(glow::TEXTURE_2D, Some(made));
                        let filter = if texture.linear { glow::LINEAR } else { glow::NEAREST } as i32;
                        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, filter);
                        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, filter);
                        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
                        gl.tex_image_2d(
                            glow::TEXTURE_2D,
                            0,
                            glow::RGBA8 as i32,
                            width as i32,
                            height as i32,
                            0,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            pixels,
                        );
                    }
                    Some([x, y]) => {
                        let Some(&target) = self.textures.get(&texture.id) else { continue };
                        gl.bind_texture(glow::TEXTURE_2D, Some(target));
                        gl.tex_sub_image_2d(
                            glow::TEXTURE_2D,
                            0,
                            x as i32,
                            y as i32,
                            width as i32,
                            height as i32,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            pixels,
                        );
                    }
                }
            }

            if !overlay.meshes.is_empty() {
                gl.viewport(0, 0, window.0 as i32, window.1 as i32);
                gl.enable(glow::BLEND);
                // premultiplied alpha over what is already drawn
                gl.blend_equation(glow::FUNC_ADD);
                gl.blend_func_separate(glow::ONE, glow::ONE_MINUS_SRC_ALPHA, glow::ONE_MINUS_DST_ALPHA, glow::ONE);
                gl.enable(glow::SCISSOR_TEST);
                gl.use_program(Some(self.program));
                if let Some(location) = gl.get_uniform_location(self.program, "size") {
                    gl.uniform_2_f32(Some(&location), window.0 as f32, window.1 as f32);
                }
                if let Some(location) = gl.get_uniform_location(self.program, "image") {
                    gl.uniform_1_i32(Some(&location), 0);
                }
                gl.active_texture(glow::TEXTURE0);
                gl.bind_vertex_array(Some(self.vertex_array));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vertices));
                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.indices));
                for mesh in &overlay.meshes {
                    let Some(&texture) = self.textures.get(&mesh.texture) else { continue };
                    let [left, top, right, bottom] = mesh.clip;
                    let (right, bottom) = (right.min(window.0), bottom.min(window.1));
                    if left >= right || top >= bottom || mesh.indices.is_empty() {
                        continue;
                    }
                    // OpenGL counts rows from the bottom
                    gl.scissor(left as i32, (window.1 - bottom) as i32, (right - left) as i32, (bottom - top) as i32);
                    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                    let vertex_bytes = std::slice::from_raw_parts(
                        mesh.vertices.as_ptr() as *const u8,
                        mesh.vertices.len() * size_of::<OverlayVertex>(),
                    );
                    let index_bytes = std::slice::from_raw_parts(mesh.indices.as_ptr() as *const u8, mesh.indices.len() * 4);
                    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, vertex_bytes, glow::STREAM_DRAW);
                    gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, index_bytes, glow::STREAM_DRAW);
                    gl.draw_elements(glow::TRIANGLES, mesh.indices.len() as i32, glow::UNSIGNED_INT, 0);
                }
                gl.bind_vertex_array(None);
                gl.disable(glow::SCISSOR_TEST);
                gl.disable(glow::BLEND);
            }

            for id in &overlay.free {
                if let Some(texture) = self.textures.remove(id) {
                    gl.delete_texture(texture);
                }
            }
        }
    }
}

unsafe fn link_program(
    gl: &glow::Context,
    vertex: &str,
    fragment: &str,
) -> Result<glow::Program, PresentError> {
    let program = unsafe { gl.create_program() }.map_err(PresentError::Backend)?;

    let mut shaders = Vec::new();
    for (kind, source) in [
        (glow::VERTEX_SHADER, vertex),
        (glow::FRAGMENT_SHADER, fragment),
    ] {
        let shader = unsafe { gl.create_shader(kind) }.map_err(PresentError::Backend)?;
        unsafe {
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let log = gl.get_shader_info_log(shader);
                return Err(PresentError::Backend(format!(
                    "shader failed to compile: {log}"
                )));
            }
            gl.attach_shader(program, shader);
        }
        shaders.push(shader);
    }

    unsafe {
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            let log = gl.get_program_info_log(program);
            return Err(PresentError::Backend(format!(
                "program failed to link: {log}"
            )));
        }
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
    }
    Ok(program)
}
