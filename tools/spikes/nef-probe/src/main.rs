//! Decodes NEF files with `rawler` and with LibRaw, compares what they return, and times a bilinear
//! demosaic (all three channels, and only the channel the exposure was lit in).
//!
//! Usage (release build, the numbers mean nothing otherwise):
//!   cargo run --release -- <file.NEF>...
//! On Windows (MSVC) LibRaw needs `CXXFLAGS=/DLIBRAW_NODLL` when it is compiled.

use std::ffi::CStr;
use std::time::Instant;

use libraw_sys as lr;

/// The crop the camera gives (x, y, width, height) in the raw frame, from rawler.
struct Crop {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

/// 0 = R, 1 = G, 2 = B for the pixel at (x, y) of an RGGB frame whose origin is a red pixel.
fn cfa(x: usize, y: usize) -> usize {
    [0, 1, 1, 2][((y & 1) << 1) | (x & 1)]
}

/// One channel of the cropped frame by bilinear interpolation, as linear f32 (black subtracted,
/// divided by the range).
fn bilinear_plane(raw: &[u16], width: usize, height: usize, crop: &Crop, channel: usize, black: f32, white: f32) -> Vec<f32> {
    let at = |x: isize, y: isize| -> f32 {
        let x = x.clamp(0, width as isize - 1) as usize;
        let y = y.clamp(0, height as isize - 1) as usize;
        f32::from(raw[y * width + x])
    };
    let scale = 1.0 / (white - black).max(1.0);
    let mut out = Vec::with_capacity(crop.w * crop.h);
    for cy in 0..crop.h {
        let ry = (crop.y + cy) as isize;
        for cx in 0..crop.w {
            let rx = (crop.x + cx) as isize;
            let here = cfa(rx as usize, ry as usize);
            let value = if here == channel {
                at(rx, ry)
            } else if channel == 1 {
                (at(rx - 1, ry) + at(rx + 1, ry) + at(rx, ry - 1) + at(rx, ry + 1)) * 0.25
            } else if here == 1 {
                // A green pixel: the wanted colour is a horizontal pair in rows that hold it, else a vertical pair.
                let red_row = ry % 2 == 0;
                let horizontal = (channel == 0) == red_row;
                if horizontal {
                    (at(rx - 1, ry) + at(rx + 1, ry)) * 0.5
                } else {
                    (at(rx, ry - 1) + at(rx, ry + 1)) * 0.5
                }
            } else {
                // The opposite colour: the four diagonal neighbours.
                (at(rx - 1, ry - 1) + at(rx + 1, ry - 1) + at(rx - 1, ry + 1) + at(rx + 1, ry + 1)) * 0.25
            };
            out.push(((value - black) * scale).max(0.0));
        }
    }
    out
}

fn percentile(sorted: &[u16], p: f64) -> u16 {
    sorted[((sorted.len() - 1) as f64 * p) as usize]
}

fn main() {
    for path in std::env::args().skip(1) {
        println!("=== {path}");
        let bytes = std::fs::read(&path).expect("read the file");

        // rawler
        let started = Instant::now();
        let raw = rawler::decode_file(&path).expect("rawler decodes");
        let rawler_ms = started.elapsed().as_secs_f64() * 1000.0;
        let rawler::RawImageData::Integer(data) = &raw.data else { panic!("float data") };
        let (w, h) = (raw.width, raw.height);
        let crop_rect = raw.crop_area.expect("a crop area");
        let crop = Crop { x: crop_rect.p.x, y: crop_rect.p.y, w: crop_rect.d.w, h: crop_rect.d.h };
        let white = raw.whitelevel.0[0] as f32;
        println!("rawler {:.1} ms | {} {} | raw {w}x{h} | crop x{} y{} {}x{} | cfa {:?} | white {white} | black {:?} | wb {:?}",
            rawler_ms, raw.make, raw.model, crop.x, crop.y, crop.w, crop.h, raw.camera.cfa, raw.blacklevel, raw.wb_coeffs);

        // Floor of each CFA plane (RGGB) in the cropped frame: the 0.01 percentile.
        for (name, ox, oy) in [("R", 0, 0), ("G1", 1, 0), ("G2", 0, 1), ("B", 1, 1)] {
            let mut plane: Vec<u16> = (crop.y..crop.y + crop.h)
                .step_by(2)
                .flat_map(|y| (crop.x..crop.x + crop.w).step_by(2).map(move |x| (x, y)))
                .map(|(x, y)| data[(y + oy) * w + x + ox])
                .collect();
            plane.sort_unstable();
            let zeros = plane.iter().take_while(|v| **v == 0).count();
            println!("  rawler plane {name}: {:.2}% of the samples are 0", zeros as f64 * 100.0 / plane.len() as f64);
            println!("  rawler plane {name}: min {} p0.01 {} median {} p99.99 {} max {}",
                plane[0], percentile(&plane, 0.0001), percentile(&plane, 0.5), percentile(&plane, 0.9999), plane[plane.len() - 1]);
        }

        // LibRaw
        unsafe {
            let started = Instant::now();
            let p = lr::libraw_init(0);
            let status = lr::libraw_open_buffer(p, bytes.as_ptr().cast(), bytes.len());
            assert_eq!(status, 0, "open_buffer");
            let status = lr::libraw_unpack(p);
            assert_eq!(status, 0, "unpack");
            let libraw_ms = started.elapsed().as_secs_f64() * 1000.0;
            let s = &(*p).sizes;
            let c = &(*p).color;
            let i = &(*p).idata;
            println!("libraw {:.1} ms (LibRaw {}) | {} {} | raw {}x{} | image {}x{} margins l{} t{} | filters {:#x} cdesc {} | black {} cblack {:?} | maximum {} data_max {} | cam_mul {:?}",
                libraw_ms, CStr::from_ptr(lr::libraw_version()).to_string_lossy(),
                CStr::from_ptr(i.make.as_ptr()).to_string_lossy(), CStr::from_ptr(i.model.as_ptr()).to_string_lossy(),
                s.raw_width, s.raw_height, s.width, s.height, s.left_margin, s.top_margin,
                i.filters, CStr::from_ptr(i.cdesc.as_ptr()).to_string_lossy(),
                c.black, &c.cblack[..4], c.maximum, c.data_maximum, c.cam_mul);
            let (rw, rh) = (s.raw_width as usize, s.raw_height as usize);
            let lib = std::slice::from_raw_parts((*p).rawdata.raw_image, rw * rh);
            if (rw, rh) == (w, h) {
                let differing = lib.iter().zip(data.iter()).filter(|(a, b)| a != b).count();
                let (mut sum_lib, mut sum_rawler) = (0f64, 0f64);
                for y in (h / 2 - 256)..(h / 2 + 256) {
                    for x in (w / 2 - 256)..(w / 2 + 256) {
                        sum_lib += f64::from(lib[y * w + x]);
                        sum_rawler += f64::from(data[y * w + x]);
                    }
                }
                let (mut max_diff, mut total_diff) = (0u32, 0u64);
                let mut histogram = [0u64; 7];
                for (a, b) in lib.iter().zip(data.iter()) {
                    let d = i32::from(*a) - i32::from(*b);
                    max_diff = max_diff.max(d.unsigned_abs());
                    total_diff += u64::from(d.unsigned_abs());
                    histogram[(d.clamp(-3, 3) + 3) as usize] += 1;
                }
                println!("  libraw minus rawler: mean |difference| {:.3}, largest {max_diff}; counts for <=-3,-2,-1,0,1,2,>=3: {histogram:?}", total_diff as f64 / lib.len() as f64);
                let n = 512.0 * 512.0;
                println!("  same raw frame size: {differing} of {} samples differ; mean of the middle 512x512: libraw {:.2}, rawler {:.2}, difference {:.2}",
                    lib.len(), sum_lib / n, sum_rawler / n, sum_lib / n - sum_rawler / n);
            } else {
                println!("  the two raw frames differ in size");
            }
            lr::libraw_close(p);
        }

        // Demosaic times, release build, one thread.
        let black = 0.0;
        let started = Instant::now();
        let all: Vec<Vec<f32>> = (0..3).map(|c| bilinear_plane(data, w, h, &crop, c, black, white)).collect();
        let full_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        let green = bilinear_plane(data, w, h, &crop, 1, black, white);
        let lit_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mean = |v: &[f32]| v.iter().map(|x| f64::from(*x)).sum::<f64>() / v.len() as f64;
        // The lit channel is the one with the highest mean.
        let lit = (0..3).max_by(|a, b| mean(&all[*a]).total_cmp(&mean(&all[*b]))).unwrap_or(0);
        // Store the lit plane the way the library stores originals: 16-bit, one channel, Deflate with predictor.
        let to_u16 = |v: &[f32]| -> Vec<u16> { v.iter().map(|x| (x * 65535.0).clamp(0.0, 65535.0) as u16).collect() };
        let plane = gtp_io::Tiff16 { width: crop.w as u32, height: crop.h as u32, channels: 1, data: to_u16(&all[lit]) };
        let tiff_path = std::env::temp_dir().join("nef-probe.tif");
        let started = Instant::now();
        gtp_io::write_tiff16(&tiff_path, &plane, gtp_io::Compression::DeflatePredictor).expect("write the plane");
        let write_ms = started.elapsed().as_secs_f64() * 1000.0;
        let size = std::fs::metadata(&tiff_path).map(|m| m.len()).unwrap_or(0);
        let _ = std::fs::remove_file(&tiff_path);
        println!("store the lit plane ({}) as a 16-bit TIFF (Deflate, predictor): {write_ms:.0} ms, {:.1} MB", ["R", "G", "B"][lit], size as f64 / 1e6);
        println!("demosaic bilinear {}x{}: all three channels {:.0} ms | one channel {:.0} ms | means R {:.4} G {:.4} B {:.4} (G alone {:.4})",
            crop.w, crop.h, full_ms, lit_ms, mean(&all[0]), mean(&all[1]), mean(&all[2]), mean(&green));
    }
}