// PLUME viewport shader.
//
// One module contains every pass used by the desktop renderer:
//   * fullscreen studio background
//   * ground plane with an anti-aliased millimetre grid
//   * depth-only shadow pass
//   * lit curve / guide geometry with procedural patterns
//   * overlay lines (guides, selection, gizmo)
//   * HDR post-processing (bloom, DOF, outline, tonemap, grain, gamma)
//
// All colours uploaded from the host are linear.  The post pass is the only
// place that encodes to sRGB, because the offscreen target is sampled by egui
// or written directly to a PNG.

struct Globals {
    view_proj: mat4x4<f32>,
    shadow_vp: mat4x4<f32>,
    camera_pos: vec4<f32>,
    camera_forward: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    bg_color: vec4<f32>,
    // x = kind (0 solid, 1 gradient, 2 studio), y = horizon, z = vignette, w = time
    bg_params: vec4<f32>,
    ground_color: vec4<f32>,
    // x = height, y = extent, z = spacing, w = grid enabled
    ground_params: vec4<f32>,
    grid_color: vec4<f32>,
    grid_major_color: vec4<f32>,
    // w = major every
    axes_color: vec4<f32>,
    ambient: vec4<f32>,
    // x = exposure multiplier, y = specular, z = rim, w = shadows enabled
    lighting: vec4<f32>,
    light_pos: array<vec4<f32>, 4>,
    // w = softness
    light_dir: array<vec4<f32>, 4>,
    // w = intensity
    light_color: array<vec4<f32>, 4>,
    // x = range, y = cos outer, z = cos inner, w = shadow bias
    light_params: array<vec4<f32>, 4>,
    // x = aspect, y = near, z = far, w = time
    view_params: vec4<f32>,
    // x = axes enabled, y = ground shadow strength, z = shadow texel, w = light count
    extra: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;

struct SceneVertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    // kind, scale_u, scale_v, spacing
    @location(4) pattern_a: vec4<f32>,
    // thickness, density, angle (radians), seed
    @location(5) pattern_b: vec4<f32>,
    // secondary colour (linear), pressure reactivity
    @location(6) pattern_c: vec4<f32>,
    // emission, channel (0 alpha, 1 colour, 2 both), selected, pressure
    @location(7) flags: vec4<f32>,
    // unlit, pattern enabled, opacity scale, unused
    @location(8) extra: vec4<f32>,
};

struct SceneOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) pattern_a: vec4<f32>,
    @location(5) pattern_b: vec4<f32>,
    @location(6) pattern_c: vec4<f32>,
    @location(7) flags: vec4<f32>,
    @location(8) extra: vec4<f32>,
};

@vertex
fn vs_scene(in: SceneVertex) -> SceneOut {
    var out: SceneOut;
    out.clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.world = in.position;
    out.normal = in.normal;
    out.uv = in.uv;
    out.color = in.color;
    out.pattern_a = in.pattern_a;
    out.pattern_b = in.pattern_b;
    out.pattern_c = in.pattern_c;
    out.flags = in.flags;
    out.extra = in.extra;
    return out;
}

// ---------------------------------------------------------------------------
// Hashing / pattern helpers.  These mirror pattern.rs closely enough that a
// brush looks the same in the viewport, exports and gallery thumbnails.
// ---------------------------------------------------------------------------

fn hash_u32(x: u32) -> u32 {
    var h = x;
    h = h ^ (h >> 16u);
    h = h * 0x7feb352du;
    h = h ^ (h >> 15u);
    h = h * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return h;
}

fn hash01(a: u32, b: u32) -> f32 {
    let h = hash_u32(a * 0x9e3779b9u ^ b * 0x85ebca6bu);
    return f32(h) / 4294967295.0;
}

fn hash2i(x: i32, y: i32) -> f32 {
    return hash01(bitcast<u32>(x), bitcast<u32>(y));
}

fn line_distance(p: vec2<f32>, angle: f32, spacing: f32) -> f32 {
    let dir = vec2<f32>(cos(angle), sin(angle));
    let normal = vec2<f32>(-dir.y, dir.x);
    let d = dot(p, normal);
    let wrapped = abs(fract(d / max(spacing, 1.0e-4)));
    return min(wrapped, 1.0 - wrapped) * spacing;
}

struct PatternSample {
    alpha: f32,
    color: vec3<f32>,
};

fn sample_pattern(
    pat_a: vec4<f32>,
    pat_b: vec4<f32>,
    pat_c: vec4<f32>,
    uv: vec2<f32>,
    pressure: f32,
) -> PatternSample {
    let kind = i32(pat_a.x + 0.5);
    let pressure_clamped = clamp(pressure, 0.0, 1.0);
    let drive = 1.0 - pat_c.w + pat_c.w * pressure_clamped;
    let p = vec2<f32>(uv.x * pat_a.y, uv.y * pat_a.z);
    let spacing = max(pat_a.w, 0.001);
    let thickness = pat_b.x;
    let density = pat_b.y;
    let angle = pat_b.z;
    let seed = u32(max(pat_b.w, 0.0));

    var result: PatternSample;
    result.alpha = 1.0;
    result.color = vec3<f32>(1.0);

    if (kind == 0) {
        return result;
    } else if (kind == 1) {
        // Halftone dots.
        let cell = p / spacing;
        let local = vec2<f32>(fract(cell.x) - 0.5, fract(cell.y) - 0.5);
        let radius = thickness * (0.25 + 0.75 * drive);
        let d = length(local);
        result.alpha = 1.0 - smoothstep(radius - 0.04, radius + 0.04, d);
    } else if (kind == 2) {
        let d = line_distance(p, angle, spacing);
        let width = thickness * spacing * (0.35 + 0.65 * drive);
        result.alpha = 1.0 - smoothstep(width - 0.02, width + 0.02, d);
    } else if (kind == 3) {
        let d1 = line_distance(p, angle, spacing);
        let d2 = line_distance(p, angle + 1.5707963, spacing);
        let width = thickness * spacing * (0.35 + 0.65 * drive);
        result.alpha = 1.0 - smoothstep(width - 0.02, width + 0.02, min(d1, d2));
    } else if (kind == 4) {
        // Stipple.
        let cell = p / spacing;
        let ix = i32(floor(cell.x));
        let iy = i32(floor(cell.y));
        let h = hash01(bitcast<u32>(ix) ^ seed, bitcast<u32>(iy));
        let keep = h < density * (0.35 + 0.65 * drive);
        let jx = hash2i(ix * 3, iy * 7);
        let jy = hash2i(ix * 11, iy * 13);
        let local = vec2<f32>(fract(cell.x), fract(cell.y))
            - (vec2<f32>(0.5, 0.5) + (vec2<f32>(jx, jy) - vec2<f32>(0.5, 0.5)) * 0.5);
        let r = thickness * (0.5 + 0.5 * drive);
        if (keep) {
            result.alpha = smoothstep(r + 0.05, r - 0.05, length(local));
        } else {
            result.alpha = 0.0;
        }
    } else if (kind == 5) {
        // Terrazzo: nearest / second nearest jittered feature point.
        let cell = p / spacing;
        let base = vec2<f32>(floor(cell.x), floor(cell.y));
        var best = 1.0e20;
        var second = 1.0e20;
        var id = 0u;
        for (var oy = -1; oy <= 1; oy = oy + 1) {
            for (var ox = -1; ox <= 1; ox = ox + 1) {
                let gx = i32(base.x) + ox;
                let gy = i32(base.y) + oy;
                let h = hash_u32(
                    bitcast<u32>(gx) * 0x9e3779b9u
                    ^ bitcast<u32>(gy) * 0x85ebca6bu
                    ^ seed
                );
                let jx = (f32(h & 0xffffu) / 65535.0 - 0.5) * 0.8;
                let jy = (f32((h >> 16u) & 0xffffu) / 65535.0 - 0.5) * 0.8;
                let feature = vec2<f32>(f32(gx) + 0.5 + jx, f32(gy) + 0.5 + jy);
                let d = length(cell - feature);
                if (d < best) {
                    second = best;
                    best = d;
                    id = h;
                } else if (d < second) {
                    second = d;
                }
            }
        }
        let border = (second - best) / spacing;
        result.alpha = smoothstep(thickness, thickness + 0.06, border);
        let tint = f32(id & 0xffu) / 255.0;
        let mix_b = f32((id >> 8u) & 0xffu) / 765.0;
        result.color = vec3<f32>(
            pat_c.x * (0.7 + 0.3 * tint),
            pat_c.y * (0.8 + 0.2 * mix_b),
            pat_c.z
        );
    }
    return result;
}

// ---------------------------------------------------------------------------
// Shading
// ---------------------------------------------------------------------------

fn shadow_factor(world: vec3<f32>, bias: f32) -> f32 {
    if (globals.lighting.w < 0.5) {
        return 1.0;
    }
    let clip = globals.shadow_vp * vec4<f32>(world, 1.0);
    if (clip.w <= 1.0e-5) {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    if (ndc.z <= 0.0 || ndc.z >= 1.0 || abs(ndc.x) > 1.02 || abs(ndc.y) > 1.02) {
        return 1.0;
    }
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let texel = max(globals.extra.z, 1.0e-5);
    var lit = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let offset = vec2<f32>(f32(x), f32(y)) * texel;
            lit = lit + textureSampleCompareLevel(
                shadow_map,
                shadow_sampler,
                uv + offset,
                ndc.z - bias
            );
        }
    }
    return lit / 9.0;
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn shade_surface(
    world: vec3<f32>,
    normal: vec3<f32>,
    view: vec3<f32>,
    albedo: vec3<f32>,
    emission: f32,
    selected: f32,
) -> vec3<f32> {
    let n = normalize(normal);
    var color = albedo * globals.ambient.rgb * globals.ambient.w;
    let count = i32(clamp(globals.extra.w, 0.0, 4.0));
    for (var i = 0; i < 4; i = i + 1) {
        if (i >= count) {
            break;
        }
        let intensity = globals.light_color[i].w;
        if (intensity <= 0.0001) {
            continue;
        }
        let kind = i32(globals.light_pos[i].w + 0.5);
        let lp = globals.light_pos[i].xyz;
        let ld = normalize(globals.light_dir[i].xyz + vec3<f32>(0.0, 0.0, 1.0e-5));
        var l = -ld;
        var attenuation = 1.0;
        if (kind == 1 || kind == 2) {
            let delta = lp - world;
            let dist = max(length(delta), 1.0e-4);
            l = delta / dist;
            attenuation = clamp(1.0 - dist / max(globals.light_params[i].x, 1.0e-3), 0.0, 1.0);
            attenuation = attenuation * attenuation;
            if (kind == 2) {
                let cone = dot(-l, ld);
                attenuation = attenuation * smoothstep(
                    globals.light_params[i].y,
                    globals.light_params[i].z,
                    cone
                );
            }
        }
        let ndotl = max(dot(n, l), 0.0);
        if (ndotl <= 0.0) {
            continue;
        }
        let h = normalize(l + max(view, vec3<f32>(0.0, 0.0, 0.001)));
        let spec = pow(max(dot(n, h), 0.0), 42.0) * globals.lighting.y;
        var shadow = 1.0;
        if (i == 0) {
            shadow = shadow_factor(world, globals.light_params[i].w + 0.0008);
        }
        let radiance = globals.light_color[i].rgb * intensity * attenuation * shadow;
        color = color + radiance * (albedo * ndotl + vec3<f32>(spec, spec, spec));
    }
    let ndotv = max(dot(n, view), 0.0);
    let rim = pow(1.0 - ndotv, 3.0) * globals.lighting.z;
    color = color + globals.ambient.rgb * rim * 0.6;
    color = color + albedo * emission;
    if (selected > 0.5) {
        let sel = vec3<f32>(1.0, 0.56, 0.18);
        color = mix(color, sel, 0.16);
        color = color + sel * pow(1.0 - ndotv, 2.0) * 0.65;
    }
    return color;
}

@fragment
fn fs_scene(in: SceneOut) -> @location(0) vec4<f32> {
    let view = normalize(globals.camera_pos.xyz - in.world);
    var albedo = in.color.rgb;
    var alpha = in.color.a * max(in.extra.z, 0.0);

    let pattern_enabled = in.extra.y > 0.5;
    if (pattern_enabled) {
        let pat = sample_pattern(in.pattern_a, in.pattern_b, in.pattern_c, in.uv, in.flags.w);
        let channel = i32(in.flags.y + 0.5);
        if (channel == 0 || channel == 2) {
            alpha = alpha * pat.alpha;
        }
        if (channel == 1 || channel == 2) {
            albedo = mix(albedo, pat.color, clamp(pat.alpha, 0.0, 1.0));
        }
    }

    var color = albedo;
    if (in.extra.x < 0.5) {
        color = shade_surface(in.world, in.normal, view, albedo, in.flags.x, in.flags.z);
    } else {
        // Guides: soft unlit fill with a fresnel edge so surfaces read in 3D.
        let n = normalize(in.normal);
        let ndotv = abs(dot(n, view));
        let edge = pow(1.0 - ndotv, 2.0);
        color = albedo * (0.82 + 0.35 * edge);
        if (in.flags.z > 0.5) {
            color = mix(color, vec3<f32>(1.0, 0.56, 0.18), 0.18)
                + vec3<f32>(1.0, 0.62, 0.22) * edge * 0.5;
        }
    }

    if (alpha <= 0.001) {
        discard;
    }
    return vec4<f32>(color, clamp(alpha, 0.0, 1.0));
}

// ---------------------------------------------------------------------------
// Depth-only shadow pass
// ---------------------------------------------------------------------------

@vertex
fn vs_shadow(in: SceneVertex) -> @builtin(position) vec4<f32> {
    return globals.shadow_vp * vec4<f32>(in.position, 1.0);
}

// ---------------------------------------------------------------------------
// Fullscreen triangle and background
// ---------------------------------------------------------------------------

struct FullscreenOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> FullscreenOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    let p = positions[index];
    var out: FullscreenOut;
    out.clip = vec4<f32>(p, 0.9999, 1.0);
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

fn hash12(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

@fragment
fn fs_background(in: FullscreenOut) -> @location(0) vec4<f32> {
    let kind = i32(globals.bg_params.x + 0.5);
    let uv = in.uv;
    var color = globals.bg_color.rgb;
    if (kind == 1) {
        color = mix(globals.bg_top.rgb, globals.bg_bottom.rgb, smoothstep(0.0, 1.0, uv.y));
    } else if (kind == 2) {
        let horizon = clamp(globals.bg_params.y, 0.02, 0.98);
        let sky = mix(globals.bg_top.rgb, globals.bg_bottom.rgb, smoothstep(0.0, 1.0, uv.y));
        // A studio sweep: darker floor below the horizon, soft reflection band at it.
        let floor = globals.bg_color.rgb * (0.55 + 0.45 * smoothstep(horizon, 1.0, uv.y));
        let blend = smoothstep(horizon - 0.18, horizon + 0.34, uv.y);
        color = mix(sky, floor, blend);
        let glow = exp(-pow((uv.y - horizon) * 8.0, 2.0));
        color = color + globals.bg_top.rgb * glow * 0.10;
    }
    let centred = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, 1.2);
    let vignette = 1.0 - globals.bg_params.z * smoothstep(0.35, 1.1, length(centred) * 1.25);
    color = color * max(vignette, 0.0);
    let grain = (hash12(uv * vec2<f32>(1920.0, 1080.0) + globals.bg_params.w) - 0.5) * 0.0015;
    return vec4<f32>(max(color + grain, vec3<f32>(0.0)), 1.0);
}

// ---------------------------------------------------------------------------
// Ground plane and grid
// ---------------------------------------------------------------------------

struct GroundOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_ground(@builtin(vertex_index) index: u32) -> GroundOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0)
    );
    let corner = corners[index];
    let extent = max(globals.ground_params.y, 100.0) * 1.8;
    let world = vec3<f32>(corner.x * extent, globals.ground_params.x, corner.y * extent);
    var out: GroundOut;
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    return out;
}

fn grid_line(value: vec2<f32>, spacing: f32) -> f32 {
    let grid = value / max(spacing, 0.001);
    let d = abs(fract(grid - vec2<f32>(0.5)) - vec2<f32>(0.5)) / max(fwidth(grid), vec2<f32>(1.0e-5));
    return 1.0 - min(min(d.x, d.y), 1.0);
}

@fragment
fn fs_ground(in: GroundOut) -> @location(0) vec4<f32> {
    let extent = max(globals.ground_params.y, 100.0);
    let dist_origin = length(in.world.xz);
    let fade = 1.0 - smoothstep(extent * 0.55, extent * 0.95, dist_origin);
    let dist_cam = length(in.world.xz - globals.camera_pos.xz);
    let near_fade = smoothstep(0.0, 20.0, dist_cam);

    var color = globals.ground_color.rgb * (0.72 + 0.28 * near_fade);
    if (globals.extra.y > 0.001 && globals.lighting.w > 0.5) {
        let lit = shadow_factor(in.world, 0.0015);
        let shadow_mix = mix(1.0, lit, clamp(globals.extra.y, 0.0, 1.0));
        color = color * (0.45 + 0.55 * shadow_mix);
    }

    if (globals.ground_params.w > 0.5) {
        let spacing = max(globals.ground_params.z, 0.001);
        let major_every = max(globals.axes_color.w, 1.0);
        let minor = grid_line(in.world.xz, spacing);
        let major = grid_line(in.world.xz, spacing * major_every);
        let minor_alpha = minor * globals.grid_color.a * fade;
        let major_alpha = major * globals.grid_major_color.a * fade;
        color = mix(color, globals.grid_color.rgb, minor_alpha);
        color = mix(color, globals.grid_major_color.rgb, major_alpha);

        if (globals.extra.x > 0.5) {
            let fw = max(fwidth(in.world.xz), vec2<f32>(1.0e-5));
            let x_axis = 1.0 - min(abs(in.world.z) / fw.y, 1.0);
            let z_axis = 1.0 - min(abs(in.world.x) / fw.x, 1.0);
            let axis_alpha = max(x_axis, z_axis) * fade * 0.9;
            let axis_color = mix(vec3<f32>(0.22, 0.46, 0.95), vec3<f32>(0.95, 0.28, 0.26), x_axis);
            color = mix(color, axis_color, axis_alpha * max(z_axis, 0.65));
        }
    }

    let alpha = globals.ground_color.a * fade;
    return vec4<f32>(color, alpha);
}

// ---------------------------------------------------------------------------
// Overlay lines
// ---------------------------------------------------------------------------

struct LineVertex {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(in: LineVertex) -> LineOut {
    var out: LineOut;
    out.clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return in.color;
}

// ---------------------------------------------------------------------------
// Post-processing
// ---------------------------------------------------------------------------

struct PostUniform {
    // x = 1/width, y = 1/height, z = width, w = height
    texel: vec4<f32>,
    // x = exposure multiplier, y = tonemap id, z = saturation, w = contrast
    grade: vec4<f32>,
    // x = lift, y = vignette, z = grain, w = sharpen
    color: vec4<f32>,
    // x = bloom strength, y = threshold, z = radius, w = soft knee
    bloom: vec4<f32>,
    // x = chromatic aberration, y = outline, z = depth of field, w = time
    effects: vec4<f32>,
    // x = focus distance, y = near, z = far, w = fog mode
    focus: vec4<f32>,
    // rgb = fog colour, w = density
    fog: vec4<f32>,
    // x = fog start, y = fog end
    fog_range: vec4<f32>,
};

@group(0) @binding(0) var<uniform> post: PostUniform;
@group(0) @binding(1) var hdr_texture: texture_2d<f32>;
@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var hdr_sampler: sampler;

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn linear_depth(d: f32) -> f32 {
    let near = max(post.focus.y, 0.0001);
    let far = max(post.focus.z, near + 0.001);
    return near * far / max(far - d * (far - near), 0.0001);
}

// Depth textures cannot be sampled with a filtering sampler, so fetch the
// texel directly and clamp to the render target bounds.
fn sample_depth(uv: vec2<f32>) -> f32 {
    let dims = vec2<i32>(max(i32(post.texel.z), 1), max(i32(post.texel.w), 1));
    let p = clamp(
        vec2<i32>(uv * vec2<f32>(f32(dims.x), f32(dims.y))),
        vec2<i32>(0, 0),
        dims - vec2<i32>(1, 1),
    );
    return textureLoad(depth_texture, p, 0);
}

fn tonemap(x: vec3<f32>, id: i32) -> vec3<f32> {
    if (id == 1) {
        return x / (vec3<f32>(1.0) + x);
    } else if (id == 2) {
        return aces(x);
    } else if (id == 3) {
        let y = max(x - vec3<f32>(0.004), vec3<f32>(0.0));
        return clamp((y * (6.2 * y + 0.5)) / (y * (6.2 * y + 1.7) + 0.06), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return clamp(x, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c < vec3<f32>(0.0031308);
    let low = c * 12.92;
    let high = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, cutoff);
}

@fragment
fn fs_post(in: FullscreenOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let texel = post.texel.xy;
    let centre = vec2<f32>(0.5, 0.5);
    let radial = uv - centre;

    // Chromatic aberration.
    let ca = post.effects.x * 0.0025;
    var scene: vec3<f32>;
    scene.r = textureSample(hdr_texture, hdr_sampler, uv + radial * ca).r;
    scene.g = textureSample(hdr_texture, hdr_sampler, uv).g;
    scene.b = textureSample(hdr_texture, hdr_sampler, uv - radial * ca).b;
    let depth = sample_depth(uv);

    // Depth of field: cheap ring blur driven by the circle of confusion.
    if (post.effects.z > 0.001) {
        let z = linear_depth(depth);
        let coc = clamp(abs(z - post.focus.x) / max(post.focus.x, 1.0), 0.0, 1.0) * post.effects.z;
        if (coc > 0.002) {
            var accum = scene;
            let radius = coc * 1.8;
            for (var i = 0; i < 8; i = i + 1) {
                let a = f32(i) * 0.785398;
                let off = vec2<f32>(cos(a), sin(a)) * radius * texel * 4.0;
                accum = accum + textureSample(hdr_texture, hdr_sampler, uv + off).rgb;
            }
            scene = accum / 9.0;
        }
    }

    // Bloom: bright-pass ring.  Twelve taps are enough for a soft studio glow.
    if (post.bloom.x > 0.001) {
        var bloom = vec3<f32>(0.0);
        var weight = 0.0;
        let threshold = post.bloom.y;
        let knee = max(post.bloom.w, 0.001);
        for (var i = 0; i < 12; i = i + 1) {
            let a = f32(i) * 0.5235988 + post.effects.w * 0.05;
            let radius = (0.35 + f32(i % 4) * 0.55) * post.bloom.z;
            let off = vec2<f32>(cos(a), sin(a)) * texel * radius * 9.0;
            let c = textureSample(hdr_texture, hdr_sampler, uv + off).rgb;
            let l = luminance(c);
            let contribution = smoothstep(threshold, threshold + knee, l);
            bloom = bloom + c * contribution;
            weight = weight + 1.0;
        }
        scene = scene + bloom * (post.bloom.x / max(weight, 1.0));
    }

    // Exposure and grading before the curve so highlights behave predictably.
    var color = scene * post.grade.x;

    // Fog.
    if (post.focus.w > 0.5) {
        let dist = linear_depth(depth);
        var fog_amount = 0.0;
        if (post.focus.w < 1.5) {
            fog_amount = clamp((dist - post.fog_range.x) / max(post.fog_range.y - post.fog_range.x, 1.0), 0.0, 1.0);
        } else {
            fog_amount = clamp(1.0 - exp(-dist * post.fog.w), 0.0, 1.0);
        }
        color = mix(color, post.fog.rgb, fog_amount * 0.85);
    }

    color = tonemap(color, i32(post.grade.y + 0.5));
    let luma = luminance(color);
    color = mix(vec3<f32>(luma), color, post.grade.z);
    color = clamp((color - vec3<f32>(0.5)) * post.grade.w + vec3<f32>(0.5) + post.color.x, vec3<f32>(0.0), vec3<f32>(1.0));

    // Unsharp mask for the final crisp look.
    if (post.color.w > 0.001) {
        let n = textureSample(hdr_texture, hdr_sampler, uv + vec2<f32>(texel.x, 0.0)).rgb * post.grade.x;
        let s = textureSample(hdr_texture, hdr_sampler, uv - vec2<f32>(texel.x, 0.0)).rgb * post.grade.x;
        let e = textureSample(hdr_texture, hdr_sampler, uv + vec2<f32>(0.0, texel.y)).rgb * post.grade.x;
        let w = textureSample(hdr_texture, hdr_sampler, uv - vec2<f32>(0.0, texel.y)).rgb * post.grade.x;
        let blur = (n + s + e + w) * 0.25;
        color = clamp(color + (color - tonemap(blur, i32(post.grade.y + 0.5))) * post.color.w, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    // Depth outline.
    if (post.effects.y > 0.001) {
        let d0 = sample_depth(uv + vec2<f32>(texel.x, 0.0));
        let d1 = sample_depth(uv - vec2<f32>(texel.x, 0.0));
        let d2 = sample_depth(uv + vec2<f32>(0.0, texel.y));
        let d3 = sample_depth(uv - vec2<f32>(0.0, texel.y));
        let edge = abs(d0 - d1) + abs(d2 - d3);
        let outline = smoothstep(0.002, 0.035, edge) * post.effects.y;
        color = mix(color, vec3<f32>(0.015, 0.018, 0.023), outline * 0.7);
    }

    // Vignette and grain.
    let vig = 1.0 - post.color.y * smoothstep(0.25, 0.85, length(radial) * 1.25);
    color = color * max(vig, 0.0);
    let grain_seed = uv * post.texel.zw + vec2<f32>(post.effects.w * 13.7, post.effects.w * 7.3);
    let grain = (hash12(grain_seed) - 0.5) * post.color.z;
    color = color + vec3<f32>(grain);

    color = linear_to_srgb(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)));
    return vec4<f32>(color, 1.0);
}
