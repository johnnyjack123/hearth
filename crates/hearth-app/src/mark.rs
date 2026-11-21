//! The product mark of the web app (`Logo`), drawn at run time for the
//! window icon: a rounded square with two rails joined by a hub. The tray
//! icon of `hearthd` draws the same.

/// Brand color (`--palette-iris-600`).
pub const BRAND: [u8; 3] = [0x54, 0x53, 0xe1];

fn distance_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let (x, y) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    (x * x + y * y).sqrt()
}

/// Color at `p` in the mark's 32-unit coordinates; `None` outside.
fn sample(p: (f32, f32), fill: [u8; 3]) -> Option<[f32; 3]> {
    let r = 9.0;
    let cx = p.0.clamp(r, 32.0 - r);
    let cy = p.1.clamp(r, 32.0 - r);
    let (dx, dy) = (p.0 - cx, p.1 - cy);
    if !(0.0..=32.0).contains(&p.0) || !(0.0..=32.0).contains(&p.1) || dx * dx + dy * dy > r * r {
        return None;
    }
    let rails = [
        ((10.0, 9.5), (10.0, 22.5)),
        ((22.0, 9.5), (22.0, 22.5)),
        ((10.0, 16.0), (22.0, 16.0)),
    ];
    let on_rail = rails
        .iter()
        .any(|(a, b)| distance_to_segment(p, *a, *b) <= 1.6);
    let hub = ((p.0 - 16.0).powi(2) + (p.1 - 16.0).powi(2)).sqrt() <= 3.0;
    Some(if on_rail || hub {
        [255.0; 3]
    } else {
        fill.map(f32::from)
    })
}

/// RGBA pixels of the mark, `size` × `size`, top row first, smoothed with
/// 4 × 4 samples per pixel.
pub fn rgba(size: usize, fill: [u8; 3]) -> Vec<u8> {
    const SAMPLES: usize = 4;
    let scale = 32.0 / size as f32;
    let mut out = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let mut color = [0.0f32; 3];
            let mut covered = 0usize;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let p = (
                        (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) * scale,
                        (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) * scale,
                    );
                    if let Some(c) = sample(p, fill) {
                        covered += 1;
                        for (sum, part) in color.iter_mut().zip(c) {
                            *sum += part;
                        }
                    }
                }
            }
            if covered == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let [r, g, b] = color.map(|sum| (sum / covered as f32).round() as u8);
            out.extend_from_slice(&[r, g, b, (255 * covered / (SAMPLES * SAMPLES)) as u8]);
        }
    }
    out
}
