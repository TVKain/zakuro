#version 450
#extension GL_GOOGLE_include_directive : require
// compiled twice, as it is and with STEREO defined, which draws the top
// screen's two pictures in one for 3D:
//   glslangValidator -V present.frag -o present.frag.spv
//   glslangValidator -V -DSTEREO present.frag -o present_stereo.frag.spv
// the filters are in present_common.glsl, which the OpenGL presenter shares.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 colour;
// the screens' images, sRGB views, so samples come in linear light
layout(set = 0, binding = 0) uniform sampler2D screen;
#ifdef STEREO
layout(set = 0, binding = 1) uniform sampler2D right_eye;
#endif
// where the screen lies in the image, its corner and size as texture
// coordinates, then the corners inset by half a texel, which samples keep
// within so filtering takes nothing from around it. the whole image when it
// holds just the screen. then how a screen is filtered, see shown, and
// whether the window encodes sRGB itself.
layout(push_constant) uniform Crop {
    layout(offset = 16) vec4 area;
    vec4 bounds;
    int mode;
    int srgb_window;
#ifdef STEREO
    // how the eyes are shown, 1 in red and cyan, 2 on alternate rows,
    // whether they swap, then where the right eye's picture lies
    int stereo;
    int swap;
    vec4 right_area;
    vec4 right_bounds;
#endif
} crop;

#include "present_common.glsl"

vec3 written(vec3 c) {
    return crop.srgb_window != 0 ? clamp(c, 0.0, 1.0) : encode_srgb(c);
}

void main() {
#ifdef STEREO
    vec3 left = shown(screen, crop.area.xy + uv * crop.area.zw, crop.bounds, crop.mode);
    vec3 right = shown(right_eye, crop.right_area.xy + uv * crop.right_area.zw, crop.right_bounds, crop.mode);
    if (crop.swap != 0) {
        vec3 other = left;
        left = right;
        right = other;
    }
    if (crop.stereo == 1) {
        // Dubois' red and cyan projection, least squares over what the
        // glasses let through, with less ghosting than the plain split. it
        // is fitted to encoded values
        left = encode_srgb(left);
        right = encode_srgb(right);
        vec3 red = vec3(dot(left, vec3(0.437, 0.449, 0.164)), dot(left, vec3(-0.062, -0.062, -0.024)), dot(left, vec3(-0.048, -0.050, -0.017)));
        vec3 cyan = vec3(dot(right, vec3(-0.011, -0.032, -0.007)), dot(right, vec3(0.377, 0.761, 0.009)), dot(right, vec3(-0.026, -0.093, 1.234)));
        vec3 both = clamp(red + cyan, 0.0, 1.0);
        colour = vec4(crop.srgb_window != 0 ? decode_srgb(both) : both, 1.0);
    } else {
        // window rows from the top, even ones the left eye's
        colour = vec4(written((int(gl_FragCoord.y) & 1) == 0 ? left : right), 1.0);
    }
#else
    colour = vec4(written(shown(screen, crop.area.xy + uv * crop.area.zw, crop.bounds, crop.mode)), 1.0);
#endif
}
