//! Mouth envelope from mono PCM.
use crate::ENVELOPE_STEP_MS;

const GATE: f32 = 0.08;
const MIN_P95_RMS: f32 = 0.003;

pub fn envelope(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let win = ((sample_rate as u64 * ENVELOPE_STEP_MS as u64) / 1000) as usize;
    if win == 0 || samples.is_empty() {
        return vec![];
    }
    let rms: Vec<f32> = samples
        .chunks(win)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let mut sorted = rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p95 = sorted[((sorted.len() - 1) as f32 * 0.95).round() as usize];
    if p95 < MIN_P95_RMS {
        return vec![0.0; rms.len()];
    }
    let step = ENVELOPE_STEP_MS as f32;
    let attack = 1.0 - (-step / 30.0).exp();
    let release = 1.0 - (-step / 80.0).exp();
    let mut y = 0.0f32;
    rms.iter()
        .map(|r| {
            let mut x = (r / p95).min(1.0);
            if x < GATE {
                x = 0.0;
            }
            y += (x - y) * if x > y { attack } else { release };
            if y < 0.01 {
                y = 0.0;
            }
            y.clamp(0.0, 1.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(sr: u32, ms: u32, amp: f32) -> Vec<f32> {
        (0..sr * ms / 1000).map(|i| amp * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / sr as f32).sin()).collect()
    }

    #[test]
    fn silence_is_zero() {
        let e = envelope(&vec![0.0; 24000], 24000);
        assert_eq!(e.len(), 20);
        assert!(e.iter().all(|v| *v == 0.0));
        assert!(envelope(&[], 24000).is_empty());
    }

    #[test]
    fn tone_then_silence() {
        let mut s = sine(24000, 1000, 0.5);
        s.extend(vec![0.0; 24000]);
        let e = envelope(&s, 24000);
        assert_eq!(e.len(), 40);
        assert!(e.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(e[10] > 0.9, "tone should open the mouth: {}", e[10]);
        assert!(e[39] < 0.05, "silence should close it: {}", e[39]);
        assert!(e[20] > e[22] && e[20] > 0.3);
    }

    #[test]
    fn level_independent() {
        let a = envelope(&sine(24000, 500, 0.1), 24000);
        let b = envelope(&sine(24000, 500, 0.8), 24000);
        assert!((a[5] - b[5]).abs() < 0.05);
    }
}
