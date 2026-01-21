//! Animation easing functions for smooth motion curves.
//!
//! ## Available Curves
//!
//! - **ease** (default), **ease-in**, **ease-out**, **ease-in-out**
//! - **sine-in**, **sine-out**, **sine-in-out** - Sinusoidal motion
//! - **quart-in**, **quart-out**, **quart-in-out** - Quartic curves
//! - **cubic-in**, **cubic-out**, **cubic-in-out** - Cubic curves
//! - **back-in**, **back-out**, **back-in-out** - Overshoot effect
//! - **expo-in**, **expo-out**, **expo-in-out** - Exponential curves
//! - **windows** - Custom curve matching Windows animations
//!
//! ## Custom Cubic Bezier
//!
//! Users can define custom curves: `cubic-bezier(x1, y1, x2, y2)`

use crate::config::Easing;
use std::f64::consts::PI;

pub fn get_easing(progress: f64, easing: &Easing) -> f64 {
  match easing {
    Easing::SineInOut => -((PI * progress).cos() - 1.0) / 2.0,
    Easing::SineIn => 1.0 - (progress * PI / 2.0).cos(),
    Easing::SineOut => (progress * PI / 2.0).sin(),
    Easing::QuartInOut => {
      if progress < 0.5 {
        8.0 * progress.powi(4)
      } else {
        1.0 - (-2.0 * progress + 2.0).powi(4) / 2.0
      }
    }
    Easing::QuartIn => progress.powi(4),
    Easing::QuartOut => 1.0 - (1.0 - progress).powi(4),
    Easing::CubicInOut => {
      if progress < 0.5 {
        4.0 * progress.powi(3)
      } else {
        1.0 - (-2.0 * progress + 2.0).powi(3) / 2.0
      }
    }
    Easing::CubicIn => progress.powi(3),
    Easing::CubicOut => 1.0 - (1.0 - progress).powi(3),
    Easing::BackInOut => {
      let c1 = 1.70158;
      let c2 = c1 * 1.525;
      if progress < 0.5 {
        ((2.0 * progress).powi(2) * ((c2 + 1.0) * 2.0 * progress - c2)) / 2.0
      } else {
        ((2.0 * progress - 2.0).powi(2) * ((c2 + 1.0) * (progress * 2.0 - 2.0) + c2) + 2.0) / 2.0
      }
    }
    Easing::BackIn => {
      let c1 = 1.70158;
      let c3 = c1 + 1.0;
      c3 * progress.powi(3) - c1 * progress.powi(2)
    }
    Easing::BackOut => {
      let c1 = 1.70158;
      let c3 = c1 + 1.0;
      1.0 + c3 * (progress - 1.0).powi(3) + c1 * (progress - 1.0).powi(2)
    }
    Easing::Ease | Easing::EaseInOut => {
      if progress < 0.5 {
        2.0 * progress * progress
      } else {
        -1.0 + (4.0 - 2.0 * progress) * progress
      }
    }
    Easing::Linear => progress,
    Easing::EaseIn => progress * progress,
    Easing::EaseOut => progress * (2.0 - progress),
    Easing::ExpoInOut => {
      if progress == 0.0 {
        0.0
      } else if progress == 1.0 {
        1.0
      } else if progress < 0.5 {
        2.0f64.powf(20.0 * progress - 10.0) / 2.0
      } else {
        (2.0 - 2.0f64.powf(-20.0 * progress + 10.0)) / 2.0
      }
    }
    Easing::ExpoIn => {
      if progress == 0.0 {
        0.0
      } else {
        2.0f64.powf(10.0 * progress - 10.0)
      }
    }
    Easing::ExpoOut => {
      if progress == 1.0 {
        1.0
      } else {
        1.0 - 2.0f64.powf(-10.0 * progress)
      }
    }
    Easing::Impulse => cubic_bezier(progress, 0.25, 0.0, 0.0, 1.0),
    Easing::Custom(x1, y1, x2, y2) => cubic_bezier(progress, *x1, *y1, *x2, *y2),
  }
}

// Cubic Bezier solver matching the JS implementation
fn cubic_bezier(x: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
  if x <= 0.0 {
    return 0.0;
  }
  if x >= 1.0 {
    return 1.0;
  }

  // Solve for t where x(t) = x inputs
  // using Newton-Raphson
  let mut t = x;
  for _ in 0..8 {
    let x_t = 3.0 * (1.0 - t).powi(2) * t * x1 + 3.0 * (1.0 - t) * t.powi(2) * x2 + t.powi(3);

    let dx_t =
      3.0 * (1.0 - 4.0 * t + 3.0 * t * t) * x1 + 3.0 * (2.0 * t - 3.0 * t * t) * x2 + 3.0 * t * t;

    if dx_t.abs() < 1e-6 {
      break;
    }
    t -= (x_t - x) / dx_t;
  }

  // Return y(t)
  3.0 * (1.0 - t).powi(2) * t * y1 + 3.0 * (1.0 - t) * t.powi(2) * y2 + t.powi(3)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_get_easing_custom() {
    // Should use the parser and return a value (not crashing)
    let val = get_easing(0.5, &Easing::Custom(0.0, 1.0, 1.0, 0.0));
    assert!(val >= 0.0 && val <= 1.0);

    // Check ease-out
    let ease_out = get_easing(0.5, &Easing::EaseOut);
    let expected = 0.5 * (2.0 - 0.5);
    assert_eq!(ease_out, expected);
  }
}
