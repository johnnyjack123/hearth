//! The tray icon, drawn at run time in the size Windows asks for: the
//! product mark of the web app (`Logo`): a rounded square with two rails
//! joined by a hub. No icon file has to ship with the program, and every
//! size is sharp.

use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject, GetDC, ReleaseDC,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

/// Colors of the mark, as sRGB.
#[derive(Clone, Copy)]
pub struct Look {
    /// Fill of the square: brand color when connected, grey otherwise.
    pub fill: [u8; 3],
    /// A dot at the lower right that asks for attention, if any.
    pub dot: Option<[u8; 3]>,
}

/// Brand color (`--palette-iris-600` of the web app).
pub const BRAND: [u8; 3] = [0x54, 0x53, 0xe1];
/// Not connected (`--palette-neutral-500`).
pub const GREY: [u8; 3] = [0x72, 0x74, 0x7d];
/// Something waits for the user (`--palette-amber-500`).
pub const AMBER: [u8; 3] = [0xeb, 0xa0, 0x02];

fn distance_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let (x, y) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    (x * x + y * y).sqrt()
}

/// Color of the mark at `p`, in its 32-unit coordinates; `None` outside.
fn sample(p: (f32, f32), look: Look) -> Option<[f32; 3]> {
    let white = [255.0, 255.0, 255.0];
    if let Some(dot) = look.dot {
        let (dx, dy) = (p.0 - 25.0, p.1 - 25.0);
        let d = (dx * dx + dy * dy).sqrt();
        if d <= 6.5 {
            return Some(dot.map(f32::from));
        }
        // A light ring keeps the dot apart from the square.
        if d <= 8.5 {
            return Some(white);
        }
    }
    // Rounded square, corner radius 9 as in the logo.
    let r = 9.0;
    let cx = p.0.clamp(r, 32.0 - r);
    let cy = p.1.clamp(r, 32.0 - r);
    let (dx, dy) = (p.0 - cx, p.1 - cy);
    if !(0.0..=32.0).contains(&p.0) || !(0.0..=32.0).contains(&p.1) || dx * dx + dy * dy > r * r {
        return None;
    }
    let half_stroke = 1.6;
    let rails = [
        ((10.0, 9.5), (10.0, 22.5)),
        ((22.0, 9.5), (22.0, 22.5)),
        ((10.0, 16.0), (22.0, 16.0)),
    ];
    let on_rail = rails
        .iter()
        .any(|(a, b)| distance_to_segment(p, *a, *b) <= half_stroke);
    let hub = ((p.0 - 16.0).powi(2) + (p.1 - 16.0).powi(2)).sqrt() <= 3.0;
    Some(if on_rail || hub {
        white
    } else {
        look.fill.map(f32::from)
    })
}

/// BGRA pixels of the mark, `size` × `size`, top row first; 4 × 4 samples
/// per pixel smooth the edges.
pub fn pixels(size: usize, look: Look) -> Vec<u8> {
    const SAMPLES: usize = 4;
    let mut out = Vec::with_capacity(size * size * 4);
    let scale = 32.0 / size as f32;
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
                    if let Some(c) = sample(p, look) {
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
            let alpha = (255 * covered / (SAMPLES * SAMPLES)) as u8;
            out.extend_from_slice(&[b, g, r, alpha]);
        }
    }
    out
}

/// An icon handle for `look` at `size` pixels; the caller destroys it.
pub fn create(size: i32, look: Look) -> HICON {
    let side = usize::try_from(size.max(16)).unwrap_or(16);
    let size = i32::try_from(side).unwrap_or(16);
    let bgra = pixels(side, look);
    // SAFETY: plain GDI calls with valid sizes; the DIB's memory is written
    // within its bounds before the icon copies it.
    unsafe {
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            // Negative: rows from the top, like `pixels`.
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let screen = GetDC(std::ptr::null_mut());
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(
            screen,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        ReleaseDC(std::ptr::null_mut(), screen);
        if color.is_null() || bits.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), bgra.len());
        // The alpha channel decides; an all-zero mask lets it.
        let mask_bytes = vec![0u8; side.div_ceil(16) * 2 * side];
        let mask = CreateBitmap(size, size, 1, 1, mask_bytes.as_ptr().cast());
        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&icon_info);
        DeleteObject(mask);
        DeleteObject(color);
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(pixels: &[u8], size: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * size + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    #[test]
    fn the_mark_has_corners_rails_and_a_dot() {
        let plain = pixels(
            32,
            Look {
                fill: BRAND,
                dot: None,
            },
        );
        // Rounded corner: transparent.
        assert_eq!(at(&plain, 32, 0, 0)[3], 0);
        // Inside the square, away from the rails: brand color (BGRA).
        assert_eq!(at(&plain, 32, 5, 16), [BRAND[2], BRAND[1], BRAND[0], 255]);
        // On the left rail and the hub: white.
        assert_eq!(at(&plain, 32, 10, 12), [255, 255, 255, 255]);
        assert_eq!(at(&plain, 32, 16, 16), [255, 255, 255, 255]);
        let alert = pixels(
            32,
            Look {
                fill: GREY,
                dot: Some(AMBER),
            },
        );
        assert_eq!(at(&alert, 32, 25, 25), [AMBER[2], AMBER[1], AMBER[0], 255]);
    }
}
