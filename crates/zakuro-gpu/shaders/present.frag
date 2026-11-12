#version 450
// compiled twice, as it is and with STEREO defined, which draws the top
// screen's two pictures in one for 3D:
//   glslc present.frag -o present.frag.spv
//   glslc -DSTEREO present.frag -o present_stereo.frag.spv
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 colour;
layout(set = 0, binding = 0) uniform sampler2D screen;
#ifdef STEREO
layout(set = 0, binding = 1) uniform sampler2D right_eye;
#endif
// where the screen lies in the image, its corner and size as texture
// coordinates, then the corners inset by half a texel, which samples keep
// within so filtering takes nothing from around it. the whole image when it
// holds just the screen. then how a screen drawn bigger than its pixels is
// filtered: 0 smooth, 1 pixels, 2 sharp, 3 xbrz.
layout(push_constant) uniform Crop {
    layout(offset = 16) vec4 area;
    vec4 bounds;
    int mode;
#ifdef STEREO
    // how the eyes are shown, 1 in red and cyan, 2 on alternate rows,
    // whether they swap, then where the right eye's picture lies
    int stereo;
    int swap;
    int unused;
    vec4 right_area;
    vec4 right_bounds;
#endif
} crop;

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

vec3 tap(sampler2D img, vec2 base, vec2 offset, vec2 size, vec4 bounds) {
    return texture(img, clamp(base + offset / size, bounds.xy, bounds.zw)).rgb;
}

vec3 xbrz_sample(sampler2D img, vec2 at, vec2 size, vec2 covered, vec4 bounds) {
    vec2 scale = 1.0 / max(covered, vec2(1e-6));
    vec2 texel = at * size;
    vec2 pos = fract(texel) - vec2(0.5, 0.5);
    vec2 coord = (floor(texel) + 0.5) / size;

    vec3 A = tap(img, coord, vec2(-1.0, -1.0), size, bounds);
    vec3 B = tap(img, coord, vec2( 0.0, -1.0), size, bounds);
    vec3 C = tap(img, coord, vec2( 1.0, -1.0), size, bounds);
    vec3 D = tap(img, coord, vec2(-1.0,  0.0), size, bounds);
    vec3 E = tap(img, coord, vec2( 0.0,  0.0), size, bounds);
    vec3 F = tap(img, coord, vec2( 1.0,  0.0), size, bounds);
    vec3 G = tap(img, coord, vec2(-1.0,  1.0), size, bounds);
    vec3 H = tap(img, coord, vec2( 0.0,  1.0), size, bounds);
    vec3 I = tap(img, coord, vec2( 1.0,  1.0), size, bounds);

    ivec4 blend = ivec4(BLEND_NONE);

    // Corner bottom-right
    if (!((eq(E, F) && eq(H, I)) || (eq(E, H) && eq(F, I)))) {
        float dist_H_F = dist_ycbcr(G, E) + dist_ycbcr(E, C) + dist_ycbcr(tap(img, coord, vec2(0.0, 2.0), size, bounds), I) + dist_ycbcr(I, tap(img, coord, vec2(2.0, 0.0), size, bounds)) + 4.0 * dist_ycbcr(H, F);
        float dist_E_I = dist_ycbcr(D, H) + dist_ycbcr(H, tap(img, coord, vec2(1.0, 2.0), size, bounds)) + dist_ycbcr(B, F) + dist_ycbcr(F, tap(img, coord, vec2(2.0, 1.0), size, bounds)) + 4.0 * dist_ycbcr(E, I);
        bool dominant = (DOMINANT_DIRECTION_THRESHOLD * dist_H_F) < dist_E_I;
        blend.z = ((dist_H_F < dist_E_I) && neq(E, F) && neq(E, H)) ? ((dominant) ? BLEND_DOMINANT : BLEND_NORMAL) : BLEND_NONE;
    }

    // Corner bottom-left
    if (!((eq(D, E) && eq(G, H)) || (eq(D, G) && eq(E, H)))) {
        float dist_G_E = dist_ycbcr(tap(img, coord, vec2(-2.0, 1.0), size, bounds), D) + dist_ycbcr(D, B) + dist_ycbcr(tap(img, coord, vec2(-1.0, 2.0), size, bounds), H) + dist_ycbcr(H, F) + 4.0 * dist_ycbcr(G, E);
        float dist_D_H = dist_ycbcr(tap(img, coord, vec2(-2.0, 0.0), size, bounds), G) + dist_ycbcr(G, tap(img, coord, vec2(0.0, 2.0), size, bounds)) + dist_ycbcr(A, E) + dist_ycbcr(E, I) + 4.0 * dist_ycbcr(D, H);
        bool dominant = (DOMINANT_DIRECTION_THRESHOLD * dist_D_H) < dist_G_E;
        blend.w = ((dist_G_E > dist_D_H) && neq(E, D) && neq(E, H)) ? ((dominant) ? BLEND_DOMINANT : BLEND_NORMAL) : BLEND_NONE;
    }

    // Corner top-right
    if (!((eq(B, C) && eq(E, F)) || (eq(B, E) && eq(C, F)))) {
        float dist_E_C = dist_ycbcr(D, B) + dist_ycbcr(B, tap(img, coord, vec2(1.0, -2.0), size, bounds)) + dist_ycbcr(H, F) + dist_ycbcr(F, tap(img, coord, vec2(2.0, -1.0), size, bounds)) + 4.0 * dist_ycbcr(E, C);
        float dist_B_F = dist_ycbcr(A, E) + dist_ycbcr(E, I) + dist_ycbcr(tap(img, coord, vec2(0.0, -2.0), size, bounds), C) + dist_ycbcr(C, tap(img, coord, vec2(2.0, 0.0), size, bounds)) + 4.0 * dist_ycbcr(B, F);
        bool dominant = (DOMINANT_DIRECTION_THRESHOLD * dist_B_F) < dist_E_C;
        blend.y = ((dist_E_C > dist_B_F) && neq(E, B) && neq(E, F)) ? ((dominant) ? BLEND_DOMINANT : BLEND_NORMAL) : BLEND_NONE;
    }

    // Corner top-left
    if (!((eq(A, B) && eq(D, E)) || (eq(A, D) && eq(B, E)))) {
        float dist_D_B = dist_ycbcr(tap(img, coord, vec2(-2.0, 0.0), size, bounds), A) + dist_ycbcr(A, tap(img, coord, vec2(0.0, -2.0), size, bounds)) + dist_ycbcr(G, E) + dist_ycbcr(E, C) + 4.0 * dist_ycbcr(D, B);
        float dist_A_E = dist_ycbcr(tap(img, coord, vec2(-2.0, -1.0), size, bounds), D) + dist_ycbcr(D, H) + dist_ycbcr(tap(img, coord, vec2(-1.0, -2.0), size, bounds), B) + dist_ycbcr(B, F) + 4.0 * dist_ycbcr(A, E);
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
            direction.x -= haveSteep ? 1.0 : 0.0;
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

// Contrast-Adaptive Sharpening (CAS) bounded strictly by local neighbor bounds
vec3 apply_cas(vec3 c, vec3 l, vec3 r, vec3 t, vec3 b, float sharpness) {
    vec3 min_c = min(c, min(min(l, r), min(t, b)));
    vec3 max_c = max(c, max(max(l, r), max(t, b)));
    vec3 contrast = max_c - min_c;
    vec3 w = -sharpness * clamp(contrast / max(max_c, vec3(1e-4)), 0.0, 1.0);
    vec3 res = (c + w * (l + r + t + b)) / (1.0 + 4.0 * w);
    return clamp(res, min_c, max_c);
}

// Presentation sampling pipeline:
// When upscaling (covered <= 1.001):
// - Mode 0 (Smooth): hardware bilinear
// - Mode 1 (Pixels): point sampling
// - Mode 2 (Sharp): sharpened step blend
// - Mode 3 (xBRZ): SOTA xBRZ-freescale with BT.2020 color metric
//
// When downscaling higher internal resolutions up to 8x (4K) to display (covered > 1.001):
// - Mode 1 (Pixels): clean nearest-neighbor downsampling
// - Mode 0 (Smooth): natural supersampling with subtle CAS (0.20)
// - Mode 2 (Sharp): 9-tap Catmull-Rom bicubic negative-lobe reconstruction + CAS (0.50)
// - Mode 3 (xBRZ): Directional edge-preserving anti-aliased tangent reconstruction + CAS (0.35)
vec3 shown(sampler2D image, vec4 area, vec4 bounds) {
    vec2 at = area.xy + uv * area.zw;
    vec2 size = vec2(textureSize(image, 0));
    vec2 covered = fwidth(at) * size;
    if (max(covered.x, covered.y) <= 1.001) {
        vec2 texel = at * size;
        if (crop.mode == 1) {
            // the nearest texel, its middle sampled
            at = (floor(texel) + 0.5) / size;
        } else if (crop.mode == 2) {
            // bilinear between texel centres, its blend squeezed into the
            // one window pixel each texel's edge falls in, at any scale
            vec2 scale = 1.0 / max(covered, vec2(1e-6));
            vec2 centred = texel - 0.5;
            vec2 blend = clamp((fract(centred) - 0.5) * scale + 0.5, 0.0, 1.0);
            at = (floor(centred) + 0.5 + blend) / size;
        } else if (crop.mode == 3) {
            return xbrz_sample(image, at, size, covered, bounds);
        }
        return texture(image, clamp(at, bounds.xy, bounds.zw)).rgb;
    }

    if (crop.mode == 1) {
        vec2 texel = (floor(at * size) + 0.5) / size;
        return texture(image, clamp(texel, bounds.xy, bounds.zw)).rgb;
    }

    vec2 px = max(fwidth(at), vec2(1e-6));
    vec3 c = texture(image, clamp(at, bounds.xy, bounds.zw)).rgb;
    vec3 l = texture(image, clamp(at - vec2(px.x, 0.0), bounds.xy, bounds.zw)).rgb;
    vec3 r = texture(image, clamp(at + vec2(px.x, 0.0), bounds.xy, bounds.zw)).rgb;
    vec3 t = texture(image, clamp(at - vec2(0.0, px.y), bounds.xy, bounds.zw)).rgb;
    vec3 b = texture(image, clamp(at + vec2(0.0, px.y), bounds.xy, bounds.zw)).rgb;

    if (crop.mode == 2) {
        // Mode 2: Sharp - 9-tap Catmull-Rom negative-lobe bicubic reconstruction + CAS
        vec2 d = px * 0.70710678;
        vec3 tl = texture(image, clamp(at + vec2(-d.x, -d.y), bounds.xy, bounds.zw)).rgb;
        vec3 tr = texture(image, clamp(at + vec2( d.x, -d.y), bounds.xy, bounds.zw)).rgb;
        vec3 bl = texture(image, clamp(at + vec2(-d.x,  d.y), bounds.xy, bounds.zw)).rgb;
        vec3 br = texture(image, clamp(at + vec2( d.x,  d.y), bounds.xy, bounds.zw)).rgb;

        vec3 cross = (l + r + t + b) * 0.25;
        vec3 diag = (tl + tr + bl + br) * 0.25;
        vec3 min_c = min(c, min(min(min(l, r), min(t, b)), min(min(tl, tr), min(bl, br))));
        vec3 max_c = max(c, max(max(max(l, r), max(t, b)), max(max(tl, tr), max(bl, br))));

        vec3 reconstructed = clamp(c * 1.5 - cross * 0.35 - diag * 0.15, min_c, max_c);
        vec3 contrast = max_c - min_c;
        vec3 w = -0.50 * clamp(contrast / max(max_c, vec3(1e-4)), 0.0, 1.0);
        vec3 sharpened = (reconstructed + w * (cross * 4.0)) / (1.0 + 4.0 * w);
        return clamp(sharpened, min_c, max_c);
    }

    if (crop.mode == 3) {
        // Mode 3: xBRZ / Edge-Preserving - Directional edge-tangent smoothing + CAS
        const vec3 luma = vec3(0.2627, 0.6780, 0.0593);
        float lum_c = dot(c, luma);
        float lum_l = dot(l, luma);
        float lum_r = dot(r, luma);
        float lum_t = dot(t, luma);
        float lum_b = dot(b, luma);

        vec2 grad = vec2(lum_r - lum_l, lum_b - lum_t);
        float edge = length(grad);

        vec3 base_color = c;
        if (edge > 0.04) {
            vec2 tangent = vec2(-grad.y, grad.x) * (px / edge) * 0.5;
            vec3 t1 = texture(image, clamp(at + tangent, bounds.xy, bounds.zw)).rgb;
            vec3 t2 = texture(image, clamp(at - tangent, bounds.xy, bounds.zw)).rgb;
            base_color = c * 0.5 + (t1 + t2) * 0.25;
        }

        return apply_cas(base_color, l, r, t, b, 0.35);
    }

    // Mode 0: Smooth - Natural high-fidelity SSAA with subtle CAS
    return apply_cas(c, l, r, t, b, 0.20);
}

void main() {
#ifdef STEREO
    vec3 left = shown(screen, crop.area, crop.bounds);
    vec3 right = shown(right_eye, crop.right_area, crop.right_bounds);
    if (crop.swap != 0) {
        vec3 other = left;
        left = right;
        right = other;
    }
    if (crop.stereo == 1) {
        // Dubois' red and cyan projection, least squares over what the
        // glasses let through, with less ghosting than the plain split
        vec3 red = vec3(dot(left, vec3(0.437, 0.449, 0.164)), dot(left, vec3(-0.062, -0.062, -0.024)), dot(left, vec3(-0.048, -0.050, -0.017)));
        vec3 cyan = vec3(dot(right, vec3(-0.011, -0.032, -0.007)), dot(right, vec3(0.377, 0.761, 0.009)), dot(right, vec3(-0.026, -0.093, 1.234)));
        colour = vec4(clamp(red + cyan, 0.0, 1.0), 1.0);
    } else {
        // window rows from the top, even ones the left eye's
        colour = vec4((int(gl_FragCoord.y) & 1) == 0 ? left : right, 1.0);
    }
#else
    colour = vec4(shown(screen, crop.area, crop.bounds), 1.0);
#endif
}
