use std::f64::consts::PI;

pub fn get_easing(progress: f64, type_: &str) -> f64 {
  match type_ {
    "sine" | "sine-in-out" | "ease-in-out-sine" => -((PI * progress).cos() - 1.0) / 2.0,
    "sine-in" | "ease-in-sine" => 1.0 - (progress * PI / 2.0).cos(),
    "sine-out" | "ease-out-sine" => (progress * PI / 2.0).sin(),
    "quart" | "quart-in-out" | "ease-in-out-quart" => {
      if progress < 0.5 {
        8.0 * progress.powi(4)
      } else {
        1.0 - (-2.0 * progress + 2.0).powi(4) / 2.0
      }
    }
    "quart-in" | "ease-in-quart" => progress.powi(4),
    "quart-out" | "ease-out-quart" => 1.0 - (1.0 - progress).powi(4),
    "cubic" | "cubic-in-out" | "ease-in-out-cubic" => {
      if progress < 0.5 {
        4.0 * progress.powi(3)
      } else {
        1.0 - (-2.0 * progress + 2.0).powi(3) / 2.0
      }
    }
    "cubic-in" | "ease-in-cubic" => progress.powi(3),
    "cubic-out" | "ease-out-cubic" => 1.0 - (1.0 - progress).powi(3),
    "back" | "back-in-out" | "ease-in-out-back" => {
      let c1 = 1.70158;
      let c2 = c1 * 1.525;
      if progress < 0.5 {
        ((2.0 * progress).powi(2) * ((c2 + 1.0) * 2.0 * progress - c2)) / 2.0
      } else {
        ((2.0 * progress - 2.0).powi(2) * ((c2 + 1.0) * (progress * 2.0 - 2.0) + c2) + 2.0) / 2.0
      }
    }
    "back-in" | "ease-in-back" => {
      let c1 = 1.70158;
      let c3 = c1 + 1.0;
      c3 * progress.powi(3) - c1 * progress.powi(2)
    }
    "back-out" | "ease-out-back" => {
      let c1 = 1.70158;
      let c3 = c1 + 1.0;
      1.0 + c3 * (progress - 1.0).powi(3) + c1 * (progress - 1.0).powi(2)
    }
    "ease" | "ease-in-out" => {
      if progress < 0.5 {
        2.0 * progress * progress
      } else {
        -1.0 + (4.0 - 2.0 * progress) * progress
      }
    }
    "linear" => progress,
    "ease-in" => progress * progress,
    "ease-out" => progress * (2.0 - progress),
    "windows" => cubic_bezier(progress, 0.25, 0.0, 0.0, 1.0),
    _ => progress * (2.0 - progress), // ease-out default
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
