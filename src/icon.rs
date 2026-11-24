//! Procedural app icon — single source of truth for the .exe resource
//! (rendered by build.rs) and the runtime window/titlebar icon.
//!
//! Design: a deep graphite squircle with an isometric block, sliced cleanly
//! along a diagonal with the two halves slightly offset. Emerald top face,
//! shaded sides, soft glow. 4×4 supersampled for crisp edges at 16–256 px.

type Rgb = [f32; 3];

const TOP_A: Rgb = [134.0, 239.0, 172.0];
const TOP_B: Rgb = [34.0, 197.0, 128.0];
const LEFT_A: Rgb = [22.0, 128.0, 86.0];
const LEFT_B: Rgb = [12.0, 84.0, 58.0];
const RIGHT_A: Rgb = [16.0, 100.0, 70.0];
const RIGHT_B: Rgb = [8.0, 58.0, 42.0];
const BG_A: Rgb = [30.0, 33.0, 42.0];
const BG_B: Rgb = [11.0, 12.0, 16.0];

pub fn render(size: u32) -> Vec<u8> {
    let s = size as f32;
    let ss = 4; // supersampling per axis
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..ss {
                for sx in 0..ss {
                    let u = (x as f32 + (sx as f32 + 0.5) / ss as f32) / s;
                    let v = (y as f32 + (sy as f32 + 0.5) / ss as f32) / s;
                    let (c, a) = shade(u, v);
                    // premultiplied accumulation
                    acc[0] += c[0] * a;
                    acc[1] += c[1] * a;
                    acc[2] += c[2] * a;
                    acc[3] += a;
                }
            }
            let n = (ss * ss) as f32;
            let a = acc[3] / n;
            let un = |k: usize| if acc[3] > 0.0 { (acc[k] / acc[3]).clamp(0.0, 255.0) } else { 0.0 };
            px.extend_from_slice(&[un(0) as u8, un(1) as u8, un(2) as u8, (a * 255.0).round() as u8]);
        }
    }
    px
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Colour + coverage (0/1) of one sample point in unit space.
fn shade(u: f32, v: f32) -> (Rgb, f32) {
    // Squircle background (superellipse, n = 5)
    let (dx, dy) = ((u - 0.5) / 0.46, (v - 0.5) / 0.46);
    if dx.abs().powf(5.0) + dy.abs().powf(5.0) > 1.0 {
        return ([0.0; 3], 0.0);
    }
    let rim = 1.0 - (dx.abs().powf(5.0) + dy.abs().powf(5.0)); // 0 at edge
    let mut col = mix(BG_A, BG_B, (u * 0.4 + v * 0.6).clamp(0.0, 1.0));
    // inner top highlight along the rim
    if rim < 0.06 && v < 0.5 {
        col = mix(col, [255.0; 3], 0.10);
    }
    // soft emerald glow behind the block
    let g = ((u - 0.5).powi(2) + (v - 0.55).powi(2)).sqrt();
    col = mix(col, [24.0, 120.0, 84.0], (1.0 - g / 0.42).clamp(0.0, 1.0).powf(2.2) * 0.55);

    // Slice: halves separated across a diagonal, shifted along its normal.
    let center = (0.5, 0.52);
    let n = (0.62f32, -0.785f32); // normal of the cut (unit-ish)
    let nl = (n.0 * n.0 + n.1 * n.1).sqrt();
    let n = (n.0 / nl, n.1 / nl);
    let side = (u - center.0) * n.0 + (v - center.1) * n.1;
    let gap = 0.016;
    if side.abs() < gap {
        return (col, 1.0);
    }
    let shift = if side > 0.0 { 0.024 } else { -0.012 };
    let (pu, pv) = (u - n.0 * shift, v - n.1 * shift);

    if let Some(face) = cube_face(pu - center.0, pv - center.1, 0.29) {
        let t = ((pv - center.1) / 0.29 + 1.0) / 2.0; // 0 top → 1 bottom
        let c = match face {
            Face::Top => mix(TOP_A, TOP_B, t * 1.6),
            Face::Left => mix(LEFT_A, LEFT_B, t),
            Face::Right => mix(RIGHT_A, RIGHT_B, t),
        };
        // bright edge on the cut for the "sliced" read
        let edge = (side.abs() - gap) < 0.012;
        let c = if edge && face == Face::Top { mix(c, [230.0, 255.0, 240.0], 0.55) } else { c };
        return (c, 1.0);
    }
    (col, 1.0)
}

#[derive(PartialEq)]
enum Face {
    Top,
    Left,
    Right,
}

/// Isometric cube (pointy-top hexagon) of circumradius `r`, centred at origin.
fn cube_face(x: f32, y: f32, r: f32) -> Option<Face> {
    let c = 0.866_025_4 * r; // cos 30°
    let t = (0.0, -r);
    let ur = (c, -r / 2.0);
    let lr = (c, r / 2.0);
    let b = (0.0, r);
    let ll = (-c, r / 2.0);
    let ul = (-c, -r / 2.0);
    let o = (0.0, 0.0);
    let p = (x, y);
    if in_quad(p, [t, ur, o, ul]) {
        Some(Face::Top)
    } else if in_quad(p, [ul, o, b, ll]) {
        Some(Face::Left)
    } else if in_quad(p, [o, ur, lr, b]) {
        Some(Face::Right)
    } else {
        None
    }
}

fn in_quad(p: (f32, f32), q: [(f32, f32); 4]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        if cross.abs() < 1e-9 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    /// `cargo test export_icon -- --ignored` refreshes packaging/skl.png.
    #[test]
    #[ignore]
    fn export_icon() {
        let px = super::render(256);
        image::save_buffer("packaging/skl.png", &px, 256, 256, image::ExtendedColorType::Rgba8).unwrap();
    }
}
