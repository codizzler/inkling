//! Easing functions map linear time `0..=1` onto eased progress `0..=1`.
//!
//! Easing shapes *how progress moves over time*; it is independent of the
//! [`RankMap`], which decides *where* a given progress value reveals.

/// A timing curve. Input and output are both clamped to `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Easing {
    #[default]
    Linear,
    /// Decelerates toward the end, good for a confident finish.
    EaseOutCubic,
    /// Strong deceleration; the reveal "lands".
    EaseOutQuint,
    /// Slow start and end, fast middle, the classic UI curve.
    EaseInOutCubic,
}

impl Easing {
    /// Apply the curve to a normalized time `t`.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            Easing::EaseOutQuint => 1.0 - (1.0 - t).powi(5),
            Easing::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_fixed() {
        for e in [
            Easing::Linear,
            Easing::EaseOutCubic,
            Easing::EaseOutQuint,
            Easing::EaseInOutCubic,
        ] {
            assert!((e.apply(0.0) - 0.0).abs() < 1e-6);
            assert!((e.apply(1.0) - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn monotonic_non_decreasing() {
        for e in [
            Easing::EaseOutCubic,
            Easing::EaseInOutCubic,
            Easing::EaseOutQuint,
        ] {
            let mut prev = -1.0;
            for i in 0..=100 {
                let v = e.apply(i as f32 / 100.0);
                assert!(v + 1e-6 >= prev, "{e:?} went backwards");
                prev = v;
            }
        }
    }
}
