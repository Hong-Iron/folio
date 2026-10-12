//! Face template matching. Embeddings are computed in Swift (Core ML);
//! only these vectors are ever stored, never images.

use serde::{Deserialize, Serialize};

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceTemplate {
    pub mean: Vec<f32>,
    pub exemplars: Vec<Vec<f32>>,
}

fn normalize(v: &[f32]) -> Option<Vec<f32>> {
    if v.is_empty() || v.iter().any(|x| !x.is_finite()) {
        return None;
    }
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    (n > 1e-6).then(|| v.iter().map(|x| x / n).collect())
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return -1.0;
    }
    match (normalize(a), normalize(b)) {
        (Some(a), Some(b)) => a.iter().zip(&b).map(|(x, y)| x * y).sum(),
        _ => -1.0,
    }
}

/// Mean of all samples plus up to `k` exemplars chosen to cover the
/// variation (farthest-point sampling), so a turned head still matches.
pub fn build_template(samples: &[Vec<f32>], k: usize) -> Option<FaceTemplate> {
    let dim = samples.first()?.len();
    let vs: Vec<Vec<f32>> = samples.iter().filter(|s| s.len() == dim).filter_map(|s| normalize(s)).collect();
    if vs.is_empty() {
        return None;
    }
    let mut sum = vec![0.0f32; dim];
    for v in &vs {
        for (s, x) in sum.iter_mut().zip(v) {
            *s += x;
        }
    }
    let mean = normalize(&sum)?;

    let mut chosen: Vec<usize> = Vec::new();
    let first = (0..vs.len()).max_by(|&a, &b| cosine(&vs[a], &mean).total_cmp(&cosine(&vs[b], &mean)))?;
    chosen.push(first);
    while chosen.len() < k.min(vs.len()) {
        let next = (0..vs.len())
            .filter(|i| !chosen.contains(i))
            .max_by(|&a, &b| {
                let da = chosen.iter().map(|&c| cosine(&vs[a], &vs[c])).fold(f32::MIN, f32::max);
                let db = chosen.iter().map(|&c| cosine(&vs[b], &vs[c])).fold(f32::MIN, f32::max);
                db.total_cmp(&da)
            })?;
        chosen.push(next);
    }
    Some(FaceTemplate { mean, exemplars: chosen.into_iter().map(|i| vs[i].clone()).collect() })
}

pub fn score(t: &FaceTemplate, emb: &[f32]) -> f32 {
    t.exemplars.iter().map(|e| cosine(e, emb)).fold(cosine(&t.mean, emb), f32::max)
}

/// Slowly follow gradual changes (haircut, lighting) from confident matches.
pub fn adapt(t: &mut FaceTemplate, emb: &[f32], rate: f32) {
    if let Some(e) = normalize(emb).filter(|e| e.len() == t.mean.len()) {
        let blended: Vec<f32> = t.mean.iter().zip(&e).map(|(m, x)| m * (1.0 - rate) + x * rate).collect();
        if let Some(m) = normalize(&blended) {
            t.mean = m;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_self_rejects_other() {
        let me: Vec<Vec<f32>> = (0..10).map(|i| vec![1.0, 0.1 * (i as f32 % 3.0), 0.05 * i as f32, 0.0]).collect();
        let t = build_template(&me, 5).unwrap();
        assert_eq!(t.exemplars.len(), 5);
        assert!(score(&t, &[1.0, 0.1, 0.2, 0.0]) > 0.9);
        assert!(score(&t, &[0.0, 0.0, 0.1, 1.0]) < 0.36);
        assert!(score(&t, &[f32::NAN, 0.0, 0.0, 0.0]) < 0.0);
    }
}
