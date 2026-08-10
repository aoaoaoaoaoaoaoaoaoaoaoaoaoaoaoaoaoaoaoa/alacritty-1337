use std::time::{Duration, Instant};

use crate::config::bell::{BellAnimation, BellConfig};

pub struct VisualBell {
    /// Visual bell animation.
    animation: BellAnimation,

    /// Visual bell duration.
    duration: Duration,

    /// The last time the visual bell rang, if at all.
    start_time: Option<Instant>,
}

impl VisualBell {
    /// Ring the visual bell, and return its intensity.
    pub fn ring(&mut self) -> f64 {
        let now = Instant::now();
        self.start_time = Some(now);
        self.intensity_at_instant(now)
    }

    /// Get the currently intensity of the visual bell. The bell's intensity
    /// ramps down from 1.0 to 0.0 at a rate determined by the bell's duration.
    pub fn intensity(&self) -> f64 {
        self.intensity_at_instant(Instant::now())
    }

    /// Check whether or not the visual bell has completed "ringing".
    pub fn completed(&mut self) -> bool {
        match self.start_time {
            Some(earlier) => {
                if Instant::now().duration_since(earlier) >= self.duration {
                    self.start_time = None;
                    true
                } else {
                    false
                }
            },
            None => true,
        }
    }

    /// Get the intensity of the visual bell at a particular instant. The bell's
    /// intensity ramps down from 1.0 to 0.0 at a rate determined by the bell's
    /// duration.
    pub fn intensity_at_instant(&self, instant: Instant) -> f64 {
        // If `duration` is zero, then the VisualBell is disabled; therefore,
        // its `intensity` is zero.
        if self.duration == Duration::from_secs(0) {
            return 0.0;
        }

        match self.start_time {
            // Similarly, if `start_time` is `None`, then the VisualBell has not
            // been "rung"; therefore, its `intensity` is zero.
            None => 0.0,

            Some(earlier) => {
                // Finally, if the `instant` at which we wish to compute the
                // VisualBell's `intensity` occurred before the VisualBell was
                // "rung", then its `intensity` is also zero.
                if instant < earlier {
                    return 0.0;
                }

                let elapsed = instant.duration_since(earlier);
                let elapsed_f = elapsed.as_secs_f64();
                let duration_f = self.duration.as_secs_f64();

                // Otherwise, we compute a value `time` from 0.0 to 1.0
                // inclusive that represents the ratio of `elapsed` time to the
                // `duration` of the VisualBell.
                let time = (elapsed_f / duration_f).min(1.0);

                // We use this to compute the inverse `intensity` of the
                // VisualBell. When `time` is 0.0, `inverse_intensity` is 0.0,
                // and when `time` is 1.0, `inverse_intensity` is 1.0.
                let inverse_intensity = match self.animation {
                    BellAnimation::Ease => cubic_bezier(0.25, 0.1, 0.25, 1.0, time),
                    BellAnimation::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, time),
                    BellAnimation::EaseOutSine => cubic_bezier(0.39, 0.575, 0.565, 1.0, time),
                    BellAnimation::EaseOutQuad => cubic_bezier(0.25, 0.46, 0.45, 0.94, time),
                    BellAnimation::EaseOutCubic => cubic_bezier(0.215, 0.61, 0.355, 1.0, time),
                    BellAnimation::EaseOutQuart => cubic_bezier(0.165, 0.84, 0.44, 1.0, time),
                    BellAnimation::EaseOutQuint => cubic_bezier(0.23, 1.0, 0.32, 1.0, time),
                    BellAnimation::EaseOutExpo => cubic_bezier(0.19, 1.0, 0.22, 1.0, time),
                    BellAnimation::EaseOutCirc => cubic_bezier(0.075, 0.82, 0.165, 1.0, time),
                    BellAnimation::Linear => time,
                };

                // Since we want the `intensity` of the VisualBell to decay over
                // `time`, we subtract the `inverse_intensity` from 1.0.
                1.0 - inverse_intensity
            },
        }
    }

    pub fn update_config(&mut self, bell_config: &BellConfig) {
        self.animation = bell_config.animation;
        self.duration = bell_config.duration();
    }
}

impl From<&BellConfig> for VisualBell {
    fn from(bell_config: &BellConfig) -> VisualBell {
        VisualBell {
            animation: bell_config.animation,
            duration: bell_config.duration(),
            start_time: None,
        }
    }
}

fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    if x <= 0. {
        return 0.;
    }
    if x >= 1. {
        return 1.;
    }

    // CSS timing functions parameterize both axes. Invert x(t) before evaluating y(t).
    let mut lower = 0.;
    let mut upper = 1.;
    for _ in 0..32 {
        let t = f64::midpoint(lower, upper);
        if bezier_axis(t, x1, x2) < x {
            lower = t;
        } else {
            upper = t;
        }
    }
    bezier_axis(f64::midpoint(lower, upper), y1, y2)
}

fn bezier_axis(t: f64, first: f64, second: f64) -> f64 {
    let inverse = 1. - t;
    3. * inverse.powi(2) * t * first + 3. * inverse * t.powi(2) * second + t.powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(clippy::float_cmp, reason = "Bezier endpoints are exact boundary contracts")]
    fn css_bezier_has_exact_endpoints() {
        assert_eq!(cubic_bezier(0.25, 0.1, 0.25, 1., 0.), 0.);
        assert_eq!(cubic_bezier(0.25, 0.1, 0.25, 1., 1.), 1.);
    }

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "distinct easing curves must not collapse at the midpoint"
    )]
    fn ease_and_ease_out_are_distinct() {
        assert_ne!(cubic_bezier(0.25, 0.1, 0.25, 1., 0.5), cubic_bezier(0., 0., 0.58, 1., 0.5));
    }

    #[test]
    fn elapsed_bell_completes_immediately() {
        let mut bell = VisualBell {
            animation: BellAnimation::Linear,
            duration: Duration::ZERO,
            start_time: Some(Instant::now()),
        };
        assert!(bell.completed());
        assert!(bell.start_time.is_none());
    }
}
