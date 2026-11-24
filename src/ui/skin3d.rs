//! Interactive 3D skin viewer: software-projected textured boxes drawn as egui
//! meshes. Drag to orbit (with inertia), wheel to zoom towards the head,
//! double-click to reset. Boxes are depth-sorted (painter's algorithm) and
//! each box's back faces are culled, which is exact for a set of convex parts.
use egui::{epaint::Vertex, pos2, vec2, Color32, Id, Mesh, Painter, Pos2, Rect, Response, TextureHandle, TextureId};

use super::anim::motion;

type V3 = [f32; 3];

#[derive(Clone, Copy)]
struct ViewState {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    zoom_target: f32,
    vel: f32,
}

const DEFAULT: ViewState = ViewState { yaw: -0.55, pitch: 0.12, zoom: 1.0, zoom_target: 1.0, vel: 0.0 };
const ZOOM_MIN: f32 = 0.7;
const ZOOM_MAX: f32 = 3.2;

struct Face {
    corners: [V3; 4], // TL, TR, BR, BL as laid out in the texture
    normal: V3,
    uv: Rect,         // texture pixels
}

struct Part {
    faces: Vec<Face>,
    center: V3,
    tex: Option<TextureId>,
    tex_size: (f32, f32), // logical (64 × h) so UVs are size-independent
}

/// Minecraft box UV unwrap at (u, v) for a w×h×d box spanning `min..max`.
fn cuboid(min: V3, max: V3, u: f32, v: f32, (w, h, d): (f32, f32, f32), mirror: bool) -> Vec<Face> {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    let r = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(pos2(x, y), vec2(w, h));
    let (mut right_uv, mut left_uv) = (r(u, v + d, d, h), r(u + d + w, v + d, d, h));
    if mirror {
        std::mem::swap(&mut right_uv, &mut left_uv);
    }
    let mut faces = vec![
        Face { corners: [[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]], normal: [0.0, 0.0, 1.0], uv: r(u + d, v + d, w, h) },
        Face { corners: [[x1, y1, z0], [x0, y1, z0], [x0, y0, z0], [x1, y0, z0]], normal: [0.0, 0.0, -1.0], uv: r(u + 2.0 * d + w, v + d, w, h) },
        Face { corners: [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]], normal: [-1.0, 0.0, 0.0], uv: right_uv },
        Face { corners: [[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]], normal: [1.0, 0.0, 0.0], uv: left_uv },
        Face { corners: [[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]], normal: [0.0, 1.0, 0.0], uv: r(u + d, v, w, d) },
        Face { corners: [[x0, y0, z1], [x1, y0, z1], [x1, y0, z0], [x0, y0, z0]], normal: [0.0, -1.0, 0.0], uv: r(u + d + w, v, w, d) },
    ];
    if mirror {
        // Reflected geometry shows the texture reversed: flip every face horizontally.
        for f in &mut faces {
            f.uv = Rect::from_min_max(pos2(f.uv.max.x, f.uv.min.y), pos2(f.uv.min.x, f.uv.max.y));
        }
    }
    faces
}

fn part(min: V3, max: V3, uv: (f32, f32), dims: (f32, f32, f32), inflate: f32, mirror: bool, tex: Option<&TextureHandle>) -> Part {
    let min = [min[0] - inflate, min[1] - inflate, min[2] - inflate];
    let max = [max[0] + inflate, max[1] + inflate, max[2] + inflate];
    let center = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, (min[2] + max[2]) / 2.0];
    let tex_size = tex.map_or((64.0, 64.0), |t| (64.0, t.size()[1] as f32 * 64.0 / t.size()[0] as f32));
    Part { faces: cuboid(min, max, uv.0, uv.1, dims, mirror), center, tex: tex.map(|t| t.id()), tex_size }
}

fn build(skin: Option<&TextureHandle>, slim: bool, cape: Option<&TextureHandle>) -> Vec<Part> {
    let aw = if slim { 3.0 } else { 4.0 };
    let legacy = skin.is_some_and(|t| t.size()[1] * 2 <= t.size()[0]);
    let mut parts = Vec::new();
    // (min, max, base uv, overlay uv, dims, overlay inflate, legacy mirror of the right limb)
    let limbs: [(V3, V3, (f32, f32), Option<(f32, f32)>, (f32, f32, f32), f32, Option<(f32, f32)>); 6] = [
        ([-4.0, 24.0, -4.0], [4.0, 32.0, 4.0], (0.0, 0.0), Some((32.0, 0.0)), (8.0, 8.0, 8.0), 0.5, None),
        ([-4.0, 12.0, -2.0], [4.0, 24.0, 2.0], (16.0, 16.0), Some((16.0, 32.0)), (8.0, 12.0, 4.0), 0.25, None),
        ([-4.0 - aw, 12.0, -2.0], [-4.0, 24.0, 2.0], (40.0, 16.0), Some((40.0, 32.0)), (aw, 12.0, 4.0), 0.25, None),
        ([4.0, 12.0, -2.0], [4.0 + aw, 24.0, 2.0], (32.0, 48.0), Some((48.0, 48.0)), (aw, 12.0, 4.0), 0.25, Some((40.0, 16.0))),
        ([-4.0, 0.0, -2.0], [0.0, 12.0, 2.0], (0.0, 16.0), Some((0.0, 32.0)), (4.0, 12.0, 4.0), 0.25, None),
        ([0.0, 0.0, -2.0], [4.0, 12.0, 2.0], (16.0, 48.0), Some((0.0, 48.0)), (4.0, 12.0, 4.0), 0.25, Some((0.0, 16.0))),
    ];
    for (i, (min, max, base, over, dims, inf, legacy_src)) in limbs.into_iter().enumerate() {
        match (legacy, legacy_src) {
            (true, Some(src)) => parts.push(part(min, max, src, dims, 0.0, true, skin)),
            _ => parts.push(part(min, max, base, dims, 0.0, false, skin)),
        }
        // Old 64×32 skins only have the hat layer.
        if let (Some(o), Some(_)) = (over, skin) {
            if !legacy || i == 0 {
                let mut p = part(min, max, o, dims, inf, false, skin);
                p.center[2] += 0.01; // overlay draws right after its base
                parts.push(p);
            }
        }
    }
    if let Some(c) = cape {
        // Built facing +z, then turned 180° so its outer side faces backwards,
        // hanging slightly away from the back.
        let mut p = part([-5.0, 8.0, -0.5], [5.0, 24.0, 0.5], (0.0, 0.0), (10.0, 16.0, 1.0), 0.0, false, Some(c));
        let tilt = 0.12f32;
        let place = |v: V3| -> V3 {
            let (x, y, z) = (-v[0], v[1], -v[2]); // 180° about y
            let dy = y - 24.0; // hinge at the shoulders
            let (s, co) = tilt.sin_cos();
            [x, 24.0 + dy * co, z + dy * s - 2.6]
        };
        for f in &mut p.faces {
            for c in &mut f.corners {
                *c = place(*c);
            }
            f.normal = [-f.normal[0], f.normal[1], -f.normal[2]];
        }
        p.center = place(p.center);
        parts.push(p);
    }
    parts
}

fn rotate(v: V3, yaw: f32, pitch: f32) -> V3 {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let x = v[0] * cy + v[2] * sy;
    let z = -v[0] * sy + v[2] * cy;
    let y = v[1] * cp - z * sp;
    let z = v[1] * sp + z * cp;
    [x, y, z]
}

/// Handles input on `resp` and paints the model inside `rect`.
pub fn show(p: &Painter, rect: Rect, resp: &Response, skin: Option<&TextureHandle>, slim: bool, cape: Option<&TextureHandle>, fallback: Color32) {
    let ctx = &resp.ctx;
    let id = Id::new("skin3d_view");
    // Dev aid: SKL_YAW sets the initial angle (screenshots of the back side).
    let mut st = ctx.data(|d| d.get_temp::<ViewState>(id)).unwrap_or_else(|| ViewState { yaw: std::env::var("SKL_YAW").ok().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT.yaw), ..DEFAULT });
    let dt = ctx.input(|i| i.stable_dt).min(0.05);

    if resp.dragged() {
        let d = resp.drag_delta();
        st.yaw += d.x * 0.011;
        st.pitch = (st.pitch + d.y * 0.008).clamp(-0.9, 0.9);
        st.vel = d.x * 0.011 / dt.max(1e-3);
    } else if st.vel.abs() > 0.01 {
        // Inertia: keep spinning after release, decaying smoothly.
        st.yaw += st.vel * dt;
        st.vel *= (-dt * 4.0).exp();
        ctx.request_repaint();
    } else {
        st.vel = 0.0;
    }
    if resp.hovered() {
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            st.zoom_target = (st.zoom_target * (scroll * 0.0025).exp()).clamp(ZOOM_MIN, ZOOM_MAX);
        }
        ctx.set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
    if resp.double_clicked() {
        st = ViewState { yaw: st.yaw.rem_euclid(std::f32::consts::TAU), ..st };
        if st.yaw > std::f32::consts::PI {
            st.yaw -= std::f32::consts::TAU;
        }
        st.zoom_target = 1.0;
        st.vel = 0.0;
        ctx.data_mut(|d| d.insert_temp(id.with("reset"), true));
    }
    // Smooth zoom and the double-click reset glide (critically damped exp approach).
    let m = motion(ctx);
    let k = if m == 0.0 { 1.0 } else { 1.0 - (-dt * 10.0 / m).exp() };
    st.zoom += (st.zoom_target - st.zoom) * k;
    if (st.zoom_target - st.zoom).abs() > 1e-3 {
        ctx.request_repaint();
    }
    if ctx.data(|d| d.get_temp::<bool>(id.with("reset"))).unwrap_or(false) {
        st.yaw += (DEFAULT.yaw - st.yaw) * k;
        st.pitch += (DEFAULT.pitch - st.pitch) * k;
        if resp.dragged() || ((st.yaw - DEFAULT.yaw).abs() < 1e-3 && (st.pitch - DEFAULT.pitch).abs() < 1e-3) {
            ctx.data_mut(|d| d.insert_temp(id.with("reset"), false));
        } else {
            ctx.request_repaint();
        }
    }
    ctx.data_mut(|d| d.insert_temp(id, st));

    // Fit the 32-unit-tall figure, then zoom towards the head.
    let scale = (rect.height() * 0.78 / 34.0).min(rect.width() * 0.8 / 20.0) * st.zoom;
    let focus = 16.0 + ((st.zoom - 1.0) / (ZOOM_MAX - 1.0)).clamp(0.0, 1.0) * 11.0;
    let center = rect.center() + vec2(0.0, 6.0);
    let project = |v: V3| -> (Pos2, f32) {
        let r = rotate([v[0], v[1] - focus, v[2]], st.yaw, st.pitch);
        (center + vec2(r[0], -r[1]) * scale, r[2])
    };

    // Ground shadow (squashed ellipse at the feet).
    let (feet, _) = project([0.0, 0.0, 0.0]);
    if rect.contains(feet) {
        p.add(egui::epaint::Shadow { offset: vec2(0.0, 0.0), blur: 18.0 * st.zoom, spread: 0.0, color: Color32::from_black_alpha(100) }
            .as_shape(Rect::from_center_size(feet, vec2(16.0 * scale, 3.0 * scale)), 3.0 * scale));
    }

    let light = {
        let l = [0.35f32, 0.75, 0.55];
        let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        [l[0] / n, l[1] / n, l[2] / n]
    };
    let mut parts = build(skin, slim, cape);
    parts.sort_by(|a, b| {
        let za = rotate([a.center[0], a.center[1] - focus, a.center[2]], st.yaw, st.pitch)[2];
        let zb = rotate([b.center[0], b.center[1] - focus, b.center[2]], st.yaw, st.pitch)[2];
        za.total_cmp(&zb)
    });
    let clip = p.with_clip_rect(rect);
    for part in &parts {
        let mut mesh = Mesh { texture_id: part.tex.unwrap_or(TextureId::default()), ..Default::default() };
        for f in &part.faces {
            let n = rotate(f.normal, st.yaw, st.pitch);
            if n[2] <= 0.0 {
                continue; // facing away
            }
            let lit = 0.62 + 0.38 * (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).max(0.0);
            let base = if part.tex.is_some() { Color32::WHITE } else { fallback };
            let col = Color32::from_rgba_premultiplied(
                (base.r() as f32 * lit) as u8,
                (base.g() as f32 * lit) as u8,
                (base.b() as f32 * lit) as u8,
                base.a(),
            );
            let (tw, th) = part.tex_size;
            let uvs = [
                pos2(f.uv.min.x / tw, f.uv.min.y / th),
                pos2(f.uv.max.x / tw, f.uv.min.y / th),
                pos2(f.uv.max.x / tw, f.uv.max.y / th),
                pos2(f.uv.min.x / tw, f.uv.max.y / th),
            ];
            let i0 = mesh.vertices.len() as u32;
            for (c, uv) in f.corners.iter().zip(uvs) {
                let uv = if part.tex.is_some() { uv } else { egui::epaint::WHITE_UV };
                mesh.vertices.push(Vertex { pos: project(*c).0, uv, color: col });
            }
            mesh.indices.extend_from_slice(&[i0, i0 + 1, i0 + 2, i0, i0 + 2, i0 + 3]);
        }
        if !mesh.is_empty() {
            clip.add(mesh);
        }
    }
}

