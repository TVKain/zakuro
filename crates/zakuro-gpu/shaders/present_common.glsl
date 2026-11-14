// the screens' filters, shared by present.frag, which Vulkan compiles, and
// the OpenGL presenter, which puts it after a header of its own, so it is
// GLSL that both 450 and 330 core take.

// the sRGB curve, for what is written to the window, and for xBRZ, whose
// colour distances are tuned to encoded values.
vec3 encode_srgb(vec3 c) {
    c = clamp(c, 0.0, 1.0);
    return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(vec3(0.0031308), c));
}

vec3 decode_srgb(vec3 c) {
    return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), step(vec3(0.04045), c));
}

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
    // xBRZ compares and blends colours as they are encoded
    return encode_srgb(textureLod(img, clamp(base + offset / size, bounds.xy, bounds.zw), 0.0).rgb);
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

    // Smart vibrance: boosts desaturated tones while protecting already saturated colors
    float luma = dot(res, vec3(0.299, 0.587, 0.114));
    float max_c = max(res.r, max(res.g, res.b));
    float min_c = min(res.r, min(res.g, res.b));
    float sat = (max_c - min_c) / max(max_c, 1e-4);
    res = mix(vec3(luma), res, 1.0 + 0.16 * (1.0 - sat));

    return res;
}


// ---------------------------------------------------------------------------
// resampling. the screens are sampled in linear light: their images are
// sRGB views, which the texture unit decodes before it blends, so every
// bilinear tap below already averages light the way the eye adds it up.

// Mitchell and Netravali's family of cubics, b and c their two parameters:
// Catmull-Rom is b 0, c 0.5, sharp; Mitchell b = c = 1/3, a little soft and
// with hardly any halo.
float cubic(float x, float b, float c) {
    x = abs(x);
    if (x < 1.0) {
        return ((12.0 - 9.0 * b - 6.0 * c) * x * x * x + (-18.0 + 12.0 * b + 6.0 * c) * x * x + (6.0 - 2.0 * b)) / 6.0;
    }
    if (x < 2.0) {
        return ((-b - 6.0 * c) * x * x * x + (6.0 * b + 30.0 * c) * x * x + (-12.0 * b - 48.0 * c) * x + (8.0 * b + 24.0 * c)) / 6.0;
    }
    return 0.0;
}

// a screen drawn bigger than its pixels with a cubic in nine bilinear taps,
// the middle two of each row and column of its four by four folded into one.
// clamped, it is held within the four texels around, so edges come out crisp
// without the halo Catmull-Rom rings with.
vec3 cubic_up(sampler2D image, vec2 at, vec2 size, vec4 bounds, float b, float c, bool clamped) {
    vec2 texel = at * size;
    vec2 p1 = floor(texel - 0.5) + 0.5;
    vec2 f = texel - p1;
    vec2 w0 = vec2(cubic(f.x + 1.0, b, c), cubic(f.y + 1.0, b, c));
    vec2 w1 = vec2(cubic(f.x, b, c), cubic(f.y, b, c));
    vec2 w2 = vec2(cubic(1.0 - f.x, b, c), cubic(1.0 - f.y, b, c));
    vec2 w3 = vec2(cubic(2.0 - f.x, b, c), cubic(2.0 - f.y, b, c));
    vec2 w12 = w1 + w2;
    vec2 t0 = clamp((p1 - 1.0) / size, bounds.xy, bounds.zw);
    vec2 t12 = clamp((p1 + w2 / w12) / size, bounds.xy, bounds.zw);
    vec2 t3 = clamp((p1 + 2.0) / size, bounds.xy, bounds.zw);
    vec3 sum = textureLod(image, vec2(t0.x, t0.y), 0.0).rgb * w0.x * w0.y;
    sum += textureLod(image, vec2(t12.x, t0.y), 0.0).rgb * w12.x * w0.y;
    sum += textureLod(image, vec2(t3.x, t0.y), 0.0).rgb * w3.x * w0.y;
    sum += textureLod(image, vec2(t0.x, t12.y), 0.0).rgb * w0.x * w12.y;
    sum += textureLod(image, vec2(t12.x, t12.y), 0.0).rgb * w12.x * w12.y;
    sum += textureLod(image, vec2(t3.x, t12.y), 0.0).rgb * w3.x * w12.y;
    sum += textureLod(image, vec2(t0.x, t3.y), 0.0).rgb * w0.x * w3.y;
    sum += textureLod(image, vec2(t12.x, t3.y), 0.0).rgb * w12.x * w3.y;
    sum += textureLod(image, vec2(t3.x, t3.y), 0.0).rgb * w3.x * w3.y;
    // the weights add up to one only nearly when b is above 0
    vec2 total = w0 + w12 + w3;
    sum = max(sum / (total.x * total.y), 0.0);
    if (!clamped) {
        return sum;
    }
    vec2 base = p1 / size;
    vec3 a = textureLod(image, clamp(base, bounds.xy, bounds.zw), 0.0).rgb;
    vec3 r = textureLod(image, clamp(base + vec2(1.0, 0.0) / size, bounds.xy, bounds.zw), 0.0).rgb;
    vec3 d = textureLod(image, clamp(base + vec2(0.0, 1.0) / size, bounds.xy, bounds.zw), 0.0).rgb;
    vec3 e = textureLod(image, clamp(base + vec2(1.0, 1.0) / size, bounds.xy, bounds.zw), 0.0).rgb;
    return clamp(sum, min(min(a, r), min(d, e)), max(max(a, r), max(d, e)));
}

// the weight a texel x texels from a window pixel's middle takes when the
// pixel covers s texels: 0 Catmull-Rom, 1 Mitchell, 2 the share of the texel
// under the pixel.
float shrinking(int kind, float x, float s) {
    if (kind == 2) {
        return max(0.0, min(x + 0.5, 0.5 * s) - max(x - 0.5, -0.5 * s));
    }
    return kind == 0 ? cubic(x / s, 0.0, 0.5) : cubic(x / s, 1.0 / 3.0, 1.0 / 3.0);
}

// how far from a window pixel's middle, in texels, the kernel reaches: it is
// stretched over the s texels the pixel covers, so that everything under it
// counts and nothing finer than the pixel aliases.
float reach(int kind, float s) {
    return kind == 2 ? 0.5 * s + 0.5 : 2.0 * s;
}

// along one axis, the tap at texel i, moving i past what it took: two
// neighbours weighing the same way share one bilinear tap placed between
// them by their weights, which with the other axis doing the same makes one
// tap of four texels, exactly what four point samples would add up to. the
// taps are worked out as they are needed rather than kept in arrays, which
// GPUs would hold in memory instead of registers, and so there is no limit
// on how many a pixel takes.
float next_tap(int kind, float p, float s, int last, inout int i, out float place) {
    float a = shrinking(kind, float(i) - p, s);
    float b = i < last ? shrinking(kind, float(i + 1) - p, s) : 0.0;
    if (a * b > 0.0) {
        place = float(i) + b / (a + b);
        i += 2;
        return a + b;
    }
    place = float(i);
    i += 1;
    return a;
}

// the most taps along a row kept at once, see shrunk
#define CACHED 8

// a screen drawn smaller than its pixels, each window pixel weighing every
// texel under and around it by the kernel. however small the window, this
// comes to about four taps for each of the image's texels in all.
vec3 shrunk(sampler2D image, vec2 at, vec2 size, vec2 covered, vec4 bounds, int kind) {
    vec2 p = at * size - 0.5;
    vec2 s = max(covered, vec2(1.0));
    vec2 r = vec2(reach(kind, s.x), reach(kind, s.y));
    ivec2 first = ivec2(floor(p - r)) + 1;
    ivec2 last = ivec2(ceil(p + r)) - 1;
    vec3 sum = vec3(0.0);
    float total = 0.0;
    // a row's taps are the same for every row: when they are few, as up to
    // a screen drawn about three and a half times smaller, they are worked
    // out once into a fixed handful, which the compiler unrolls into
    // registers, instead of again for each row
    float px[CACHED];
    float wx[CACHED];
    int x = first.x;
    for (int k = 0; k < CACHED; k++) {
        px[k] = 0.0;
        wx[k] = 0.0;
        if (x <= last.x) {
            wx[k] = next_tap(kind, p.x, s.x, last.x, x, px[k]);
        }
    }
    if (x > last.x) {
        float across = 0.0;
        for (int k = 0; k < CACHED; k++) {
            across += wx[k];
        }
        for (int y = first.y; y <= last.y;) {
            float py;
            float wy = next_tap(kind, p.y, s.y, last.y, y, py);
            if (wy == 0.0) {
                continue;
            }
            float v = clamp((py + 0.5) / size.y, bounds.y, bounds.w);
            vec3 row = vec3(0.0);
            for (int k = 0; k < CACHED; k++) {
                if (wx[k] != 0.0) {
                    float u = clamp((px[k] + 0.5) / size.x, bounds.x, bounds.z);
                    row += textureLod(image, vec2(u, v), 0.0).rgb * wx[k];
                }
            }
            sum += row * wy;
            total += across * wy;
        }
        return max(sum / total, 0.0);
    }
    int y = first.y;
    while (y <= last.y) {
        float py;
        float wy = next_tap(kind, p.y, s.y, last.y, y, py);
        if (wy == 0.0) {
            continue;
        }
        float v = clamp((py + 0.5) / size.y, bounds.y, bounds.w);
        vec3 row = vec3(0.0);
        float across = 0.0;
        int x = first.x;
        while (x <= last.x) {
            float px;
            float wx = next_tap(kind, p.x, s.x, last.x, x, px);
            if (wx != 0.0) {
                float u = clamp((px + 0.5) / size.x, bounds.x, bounds.z);
                row += textureLod(image, vec2(u, v), 0.0).rgb * wx;
                across += wx;
            }
        }
        sum += row * wy;
        total += across * wy;
    }
    return max(sum / total, 0.0);
}

// A screen filtered as asked: 0 soft, 1 pixelated, 2 sharp, 3 xBRZ, 4 xBRZ
// blended with bilinear, at in the image's texture coordinates, its samples
// kept within bounds, in linear light. drawn bigger than its pixels, soft is
// a Mitchell cubic, sharp a Catmull-Rom held to the texels around, pixelated
// sharp bilinear and xBRZ follows its edges as lines. drawn smaller, as at
// higher internal resolutions, every window pixel weighs all it covers:
// soft with Mitchell, pixelated by area, the rest with Catmull-Rom.
// measured against a frame drawn at 8 times and reduced with Lanczos, or by
// area, those come closest, and without blurring.
vec3 shown(sampler2D image, vec2 at, vec4 bounds, int mode) {
    vec2 size = vec2(textureSize(image, 0));
    vec2 covered = fwidth(at) * size;
    if (max(covered.x, covered.y) > 1.001) {
        return shrunk(image, at, size, covered, bounds, mode == 0 ? 1 : mode == 1 ? 2 : 0);
    }
    if (mode == 0) {
        return cubic_up(image, at, size, bounds, 1.0 / 3.0, 1.0 / 3.0, false);
    }
    if (mode == 2) {
        return cubic_up(image, at, size, bounds, 0.0, 0.5, true);
    }
    if (mode == 1) {
        // bilinear between texel centres, its blend squeezed into the one
        // window pixel each texel's edge falls in: square pixels all the
        // same size at any scale, their edges without jaggies
        vec2 scale = 1.0 / max(covered, vec2(1e-6));
        vec2 centred = at * size - 0.5;
        vec2 blend = clamp((fract(centred) - 0.5) * scale + 0.5, 0.0, 1.0);
        return textureLod(image, clamp((floor(centred) + 0.5 + blend) / size, bounds.xy, bounds.zw), 0.0).rgb;
    }
    vec3 lines = xbrz_sample(image, at, size, covered, bounds);
    if (mode == 3) {
        return decode_srgb(lines);
    }
    vec3 lin = encode_srgb(textureLod(image, clamp(at, bounds.xy, bounds.zw), 0.0).rgb);
    float luma = dot(lin, vec3(0.299, 0.587, 0.114));
    float max_c = max(lin.r, max(lin.g, lin.b));
    float min_c = min(lin.r, min(lin.g, lin.b));
    float sat = (max_c - min_c) / max(max_c, 1e-4);
    lin = mix(vec3(luma), lin, 1.0 + 0.16 * (1.0 - sat));
    return decode_srgb(clamp(mix(lines, lin, 0.40), 0.0, 1.0));
}
