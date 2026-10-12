//! Face embeddings with SFace (OpenCV Zoo, Apache-2.0), run on the CPU by
//! tract. Only called at session start and every couple of minutes, so a
//! few tens of milliseconds per call costs next to nothing.
//!
//! The input pipeline matches OpenCV's FaceRecognizerSF: a similarity
//! transform maps five landmarks onto the ArcFace 112×112 template, the
//! crop is resampled bilinearly, and RGB values 0–255 go in as NCHW floats.

use tract_onnx::prelude::*;

use crate::error::{NdError, Result};
use crate::signal::Point;

pub const SIZE: usize = 112;

/// Template positions, in order: image-left eye, image-right eye, nose tip,
/// image-left mouth corner, image-right mouth corner.
pub const TEMPLATE: [(f32, f32); 5] = [
    (38.2946, 51.6963),
    (73.5318, 51.5014),
    (56.0252, 71.7366),
    (41.5493, 92.3655),
    (70.7299, 92.2041),
];

/// Least-squares similarity transform (scale, rotation, translation) from
/// `src` to `dst`: x' = a·x − b·y + tx, y' = b·x + a·y + ty.
pub fn similarity(src: &[(f32, f32)], dst: &[(f32, f32)]) -> Option<[f32; 4]> {
    if src.len() != dst.len() || src.len() < 2 {
        return None;
    }
    let n = src.len() as f32;
    let (msx, msy) = (src.iter().map(|p| p.0).sum::<f32>() / n, src.iter().map(|p| p.1).sum::<f32>() / n);
    let (mdx, mdy) = (dst.iter().map(|p| p.0).sum::<f32>() / n, dst.iter().map(|p| p.1).sum::<f32>() / n);
    let (mut num_a, mut num_b, mut den) = (0.0f32, 0.0f32, 0.0f32);
    for (s, d) in src.iter().zip(dst) {
        let (sx, sy, dx, dy) = (s.0 - msx, s.1 - msy, d.0 - mdx, d.1 - mdy);
        num_a += dx * sx + dy * sy;
        num_b += dy * sx - dx * sy;
        den += sx * sx + sy * sy;
    }
    if den <= 1e-6 || !den.is_finite() {
        return None;
    }
    let (a, b) = (num_a / den, num_b / den);
    Some([a, b, mdx - (a * msx - b * msy), mdy - (b * msx + a * msy)])
}

/// Warps an RGBA image (top-left origin) into the 112×112 aligned crop as
/// planar RGB floats. Pixels outside the source are black, like OpenCV.
pub fn align(rgba: &[u8], width: usize, height: usize, landmarks: &[(f32, f32)]) -> Option<Vec<f32>> {
    if landmarks.len() != 5 || rgba.len() < width * height * 4 || width == 0 || height == 0 {
        return None;
    }
    let [a, b, tx, ty] = similarity(landmarks, &TEMPLATE)?;
    let det = a * a + b * b;
    if det <= 1e-9 {
        return None;
    }
    let mut out = vec![0.0f32; 3 * SIZE * SIZE];
    let px = |x: usize, y: usize, c: usize| rgba[(y * width + x) * 4 + c] as f32;
    for v in 0..SIZE {
        for u in 0..SIZE {
            // Inverse map: source = M⁻¹ · (dest − t).
            let (du, dv) = (u as f32 - tx, v as f32 - ty);
            let sx = (a * du + b * dv) / det;
            let sy = (-b * du + a * dv) / det;
            if sx < 0.0 || sy < 0.0 || sx > (width - 1) as f32 || sy > (height - 1) as f32 {
                continue;
            }
            let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
            let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
            for c in 0..3 {
                let top = px(x0, y0, c) * (1.0 - fx) + px(x1, y0, c) * fx;
                let bottom = px(x0, y1, c) * (1.0 - fx) + px(x1, y1, c) * fx;
                out[c * SIZE * SIZE + v * SIZE + u] = top * (1.0 - fy) + bottom * fy;
            }
        }
    }
    Some(out)
}

pub struct FaceNet {
    plan: TypedRunnableModel<TypedModel>,
}

fn err(e: impl std::fmt::Display) -> NdError {
    NdError::Invalid { msg: format!("face model: {e}") }
}

impl FaceNet {
    pub fn load(path: &str) -> Result<Self> {
        let plan = tract_onnx::onnx()
            .model_for_path(path)
            .map_err(err)?
            .with_input_fact(0, f32::fact([1, 3, SIZE, SIZE]).into())
            .map_err(err)?
            .into_optimized()
            .map_err(err)?
            .into_runnable()
            .map_err(err)?;
        Ok(Self { plan })
    }

    /// Embedding of an aligned crop (planar RGB, 0–255).
    pub fn embed_aligned(&self, planar: Vec<f32>) -> Result<Vec<f32>> {
        if planar.len() != 3 * SIZE * SIZE {
            return Err(NdError::Invalid { msg: "aligned crop has the wrong size".into() });
        }
        let input: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, SIZE, SIZE), planar).map_err(err)?.into();
        let out = self.plan.run(tvec!(input.into())).map_err(err)?;
        let v: Vec<f32> = out[0].to_array_view::<f32>().map_err(err)?.iter().copied().collect();
        if v.is_empty() || v.iter().any(|x| !x.is_finite()) {
            return Err(NdError::Invalid { msg: "face model returned an unusable embedding".into() });
        }
        Ok(v)
    }

    pub fn embed(&self, rgba: &[u8], width: usize, height: usize, landmarks: &[Point]) -> Result<Vec<f32>> {
        let pts: Vec<(f32, f32)> = landmarks.iter().map(|p| (p.x, p.y)).collect();
        if pts.iter().any(|p| !p.0.is_finite() || !p.1.is_finite()) {
            return Err(NdError::Invalid { msg: "landmarks aren't finite".into() });
        }
        let crop = align(rgba, width, height, &pts).ok_or(NdError::Invalid { msg: "couldn't align the face".into() })?;
        self.embed_aligned(crop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::cosine;

    #[test]
    fn similarity_recovers_a_known_transform() {
        let (a, b, tx, ty) = (0.8f32 * 0.3f32.cos(), 0.8f32 * 0.3f32.sin(), 12.0, -7.0);
        let src: Vec<(f32, f32)> = TEMPLATE.iter().map(|p| (p.0 * 2.0 + 100.0, p.1 * 2.0 + 50.0)).collect();
        let dst: Vec<(f32, f32)> = src.iter().map(|&(x, y)| (a * x - b * y + tx, b * x + a * y + ty)).collect();
        let m = similarity(&src, &dst).unwrap();
        for (got, want) in m.iter().zip([a, b, tx, ty]) {
            assert!((got - want).abs() < 1e-3, "{m:?}");
        }
        assert!(similarity(&[(1.0, 1.0); 5], &TEMPLATE).is_none(), "degenerate points");
    }

    /// A face-like test card: landmarks drawn as colored dots on gray.
    fn card(width: usize, height: usize, scale: f32, ox: f32, oy: f32) -> (Vec<u8>, Vec<(f32, f32)>) {
        let lm: Vec<(f32, f32)> = TEMPLATE.iter().map(|p| (p.0 * scale + ox, p.1 * scale + oy)).collect();
        let mut img = vec![128u8; width * height * 4];
        for (i, &(cx, cy)) in lm.iter().enumerate() {
            for y in 0..height {
                for x in 0..width {
                    if ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() < 4.0 * scale {
                        let o = (y * width + x) * 4;
                        img[o] = (40 * i) as u8;
                        img[o + 1] = 255 - (40 * i) as u8;
                        img[o + 2] = 200;
                    }
                }
            }
        }
        (img, lm)
    }

    #[test]
    fn alignment_lands_landmarks_on_the_template() {
        let (img, lm) = card(640, 480, 2.5, 180.0, 90.0);
        let crop = align(&img, 640, 480, &lm).unwrap();
        // Each landmark's dot should now sit at its template position.
        for (i, &(tx, ty)) in TEMPLATE.iter().enumerate() {
            let at = (ty.round() as usize) * SIZE + tx.round() as usize;
            assert!((crop[at] - (40 * i) as f32).abs() < 30.0, "landmark {i}: red {}", crop[at]);
            assert!((crop[2 * SIZE * SIZE + at] - 200.0).abs() < 30.0, "landmark {i}: blue");
        }
    }

    fn model_path() -> Option<String> {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../models/face_recognition_sface_2021dec.onnx");
        std::path::Path::new(p).exists().then(|| p.to_string())
    }

    #[test]
    fn model_runs_and_is_stable() {
        let Some(path) = model_path() else {
            eprintln!("face model not downloaded; skipping (scripts/fetch-models.sh)");
            return;
        };
        let net = FaceNet::load(&path).unwrap();
        let (img, lm) = card(640, 480, 2.5, 180.0, 90.0);
        let pts: Vec<Point> = lm.iter().map(|&(x, y)| Point { x, y }).collect();
        let e1 = net.embed(&img, 640, 480, &pts).unwrap();
        assert_eq!(e1.len(), 128);
        // The same content at another position and scale aligns to the same crop.
        let (img2, lm2) = card(640, 480, 2.0, 260.0, 160.0);
        let pts2: Vec<Point> = lm2.iter().map(|&(x, y)| Point { x, y }).collect();
        let e2 = net.embed(&img2, 640, 480, &pts2).unwrap();
        assert!(cosine(&e1, &e2) > 0.95, "alignment makes position and scale irrelevant: {}", cosine(&e1, &e2));
        let t = std::time::Instant::now();
        for _ in 0..5 {
            net.embed(&img, 640, 480, &pts).unwrap();
        }
        eprintln!("embedding: {:.1} ms each", t.elapsed().as_secs_f64() * 1000.0 / 5.0);
    }
}
