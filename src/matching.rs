//! Fuzzy window matching algorithm for discovering managed windows.
//!
//! This module provides the core algorithm for matching user-specified window class
//! patterns against system windows. It's used by both Linux (KWin) and Windows (Win32)
//! backends during window discovery.
//!
//! ## Algorithm (Weighted Fuzzy Subsequence)
//!
//! The matching uses a weighted scoring system to find the best window:
//!
//! | Match Type | Score |
//! |------------|-------|
//! | Exact match | 10,000 points |
//! | Substring match | 5,000 points |
//! | Subsequence match | 1,000 base + bonuses |
//!
//! ### Bonuses and Penalties
//!
//! - **Boundary bonus**: +300 for start of string, +250 after separator (`.`, `-`, `_`, ` `)
//! - **Consecutive bonus**: +100 per consecutive character match (compounding)
//! - **Gap penalty**: -50 per skipped character
//! - **Visibility bonus**: +2,000 for visible windows
//! - **Managed bonus**: +1,000 for already-managed windows (prefer reusing)
//!
//! ## Threshold
//!
//! A minimum score of 500 is required to prevent weak matches (e.g., single-letter
//! coincidences from matching unrelated windows).

// =============================================================================
// Scoring Constants
// =============================================================================

/// Score for exact case-insensitive match (e.g., "wezterm" matches "wezterm")
const SCORE_EXACT_MATCH: i32 = 10000;

/// Score for substring match (e.g., "term" matches "wezterm")
const SCORE_SUBSTRING_MATCH: i32 = 5000;

/// Base score for matching all characters as subsequence
const SCORE_SUBSEQUENCE_BASE: i32 = 1000;

/// Bonus for match at start of string
const BONUS_BOUNDARY_START: i32 = 300;

/// Bonus for match after separator character
const BONUS_BOUNDARY_SEPARATOR: i32 = 250;

/// Bonus per consecutive character match (multiplied by streak count)
const BONUS_CONSECUTIVE: i32 = 100;

/// Penalty per skipped character in subsequence matching
const PENALTY_GAP: i32 = 50;

/// Bonus for visible windows
const BONUS_VISIBILITY: i32 = 1000;

/// Bonus for windows already managed by janq
const BONUS_MANAGED: i32 = 3000;

/// Minimum score threshold to accept a match
const THRESHOLD_MINIMUM: i32 = 500;

// =============================================================================
// Types
// =============================================================================

/// Represents a discovered window from the system.
///
/// Used by the fuzzy matching algorithm to find the best window for a given
/// `window_class` configuration value.
#[derive(Clone, Debug, Default)]
pub struct FoundWindow {
  pub id: String, // HWND for Windows, KWin ID for Linux
  pub class_lowercase: String,
  pub proc_lowercase: String,
  pub pid: u32,
  pub is_visible: bool,
  pub is_managed: bool, // Pre-calculated during discovery
}

// =============================================================================
// Helper Utilities
// =============================================================================

#[cfg(target_os = "windows")]
pub fn u16_contains_ascii_ignore_case(haystack: &[u16], needle: &str) -> bool {
  let needle_bytes = needle.as_bytes();
  if haystack.len() < needle_bytes.len() {
    return false;
  }
  haystack.windows(needle_bytes.len()).any(|window| {
    window.iter().zip(needle_bytes).all(|(&h, &n)| {
      let h_lower = if h <= 127 {
        (h as u8).to_ascii_lowercase() as u16
      } else {
        h
      };
      h_lower == n.to_ascii_lowercase() as u16
    })
  })
}

#[cfg(target_os = "windows")]
pub fn u16_eq_ascii_ignore_case(haystack: &[u16], needle: &str) -> bool {
  let needle_bytes = needle.as_bytes();
  if haystack.len() != needle_bytes.len() {
    return false;
  }
  haystack.iter().zip(needle_bytes).all(|(&h, &n)| {
    let h_lower = if h <= 127 {
      (h as u8).to_ascii_lowercase() as u16
    } else {
      h
    };
    h_lower == n.to_ascii_lowercase() as u16
  })
}

/// Calculates the Levenshtein distance between two strings using a stack-based buffer.
fn levenshtein_distance(a: &str, b: &str) -> usize {
  let b_len = b.chars().count();
  if b_len == 0 {
    return a.chars().count();
  }
  if b_len > 64 {
    return 64; // Cap for safety, flags/apps aren't this long
  }

  let mut row = [0usize; 65];
  for i in 0..=b_len {
    row[i] = i;
  }

  for (i, ca) in a.chars().enumerate() {
    let mut prev = i + 1;
    for (j, cb) in b.chars().enumerate() {
      let cost = if ca == cb { 0 } else { 1 };
      let current = (row[j + 1] + 1).min(prev + 1).min(row[j] + cost);
      row[j] = prev;
      prev = current;
    }
    row[b_len] = prev;
  }
  row[b_len]
}

// =============================================================================
// Matching Algorithm
// =============================================================================

/// Finds the best matching window for a target class/process name.
///
/// # Arguments
/// * `target` - The window_class from user config to search for
/// * `candidates` - List of windows discovered from the system
///
/// # Returns
/// The highest-scoring window, or None if no candidate scores above threshold.
pub fn fuzzy_match_window(target: &str, candidates: &[FoundWindow]) -> Option<FoundWindow> {
  let lower_target = target.to_lowercase();
  if lower_target.is_empty() {
    return None;
  }

  let mut best_score = THRESHOLD_MINIMUM;
  let mut best_win = None;

  for win in candidates {
    let mut score = 0;

    // 1. Check class_name and proc_name
    for haystack in &[&win.class_lowercase, &win.proc_lowercase] {
      if haystack.is_empty() {
        continue;
      }

      let haystack_score = score_haystack_lowercased(&lower_target, haystack);
      score = score.max(haystack_score);
    }

    if score <= 0 {
      continue;
    }

    // 2. Priority Boosts
    if win.is_visible {
      score += BONUS_VISIBILITY;
    }

    if win.is_managed {
      score += BONUS_MANAGED;
    }

    if score > best_score {
      best_score = score;
      best_win = Some(win.clone());
    }
  }

  best_win
}

/// Scores how well a lowercased haystack (window class/process name) matches the target.
fn score_haystack_lowercased(lower_target: &str, lower_haystack: &str) -> i32 {
  // 1. Exact match
  if lower_haystack == lower_target {
    return SCORE_EXACT_MATCH;
  }

  // 2. Substring match
  if lower_haystack.contains(lower_target) {
    return SCORE_SUBSTRING_MATCH;
  }

  // 3. Fuzzy subsequence matching
  let subseq_score = score_subsequence(lower_target, lower_haystack);
  if subseq_score > 0 {
    return subseq_score;
  }

  // 4. Edit distance for typos (lower priority than subsequence)
  let dist = levenshtein_distance(lower_target, lower_haystack);
  let typo_score = 900 - (dist as i32 * 100);
  typo_score.max(0)
}

/// Scores how well a haystack (window class/process name) matches the target.
/// This version lowercases the haystack, which may allocate.
pub fn score_haystack(lower_target: &str, haystack: &str) -> i32 {
  let lower_haystack = haystack.to_lowercase();
  score_haystack_lowercased(lower_target, &lower_haystack)
}

/// Scores a fuzzy subsequence match with boundary/gap penalties.
fn score_subsequence(needle: &str, haystack: &str) -> i32 {
  let mut score = 0;
  let mut h_idx = 0;
  let mut last_match_idx: i32 = -1;
  let mut consecutive_count = 0;
  let mut matches = 0;

  for n_char in needle.chars() {
    let mut found = false;
    let search_slice = &haystack[h_idx..];

    for (rel_idx, h_char) in search_slice.char_indices() {
      if h_char == n_char {
        let abs_idx = h_idx + rel_idx;
        matches += 1;

        // Bonus: Boundary (start of string or follows separator)
        if abs_idx == 0 {
          score += BONUS_BOUNDARY_START;
        } else {
          let prev_char = haystack.as_bytes().get(abs_idx - 1).copied().unwrap_or(0);
          if prev_char == b'.' || prev_char == b'-' || prev_char == b'_' || prev_char == b' ' {
            score += BONUS_BOUNDARY_SEPARATOR;
          }
        }

        // Bonus: Consecutive
        if last_match_idx != -1 && abs_idx == (last_match_idx as usize) + 1 {
          consecutive_count += 1;
          score += BONUS_CONSECUTIVE * consecutive_count;
        } else {
          consecutive_count = 0;
          // Penalty: Gap
          if last_match_idx != -1 {
            let gap = abs_idx - (last_match_idx as usize) - 1;
            score -= (gap as i32) * PENALTY_GAP;
          }
        }

        last_match_idx = abs_idx as i32;
        h_idx = abs_idx + h_char.len_utf8();
        found = true;
        break;
      }
    }

    if !found {
      // Entire needle must be found as subsequence
      return 0;
    }
  }

  // Base score for matching all letters
  if matches == needle.chars().count() {
    score += SCORE_SUBSEQUENCE_BASE;
  }

  score
}

// =============================================================================
// Suggestion Helper for Config Validation
// =============================================================================

/// Minimum score to suggest a correction (lower threshold than window matching
/// since we're matching shorter strings like "activ" -> "active")
const SUGGESTION_THRESHOLD: i32 = 300;

/// Suggests the most similar option from a list of valid options.
///
/// Uses the same fuzzy matching algorithm as window matching, but tuned for
/// short strings like configuration values.
///
/// # Arguments
/// * `input` - The user's (invalid) input
/// * `valid_options` - Slice of valid option strings to match against
///
/// # Returns
/// The best matching option if score is above threshold, otherwise None.
///
/// # Example
/// ```ignore
/// let options = &["follow-mouse", "active", "specific"];
/// assert_eq!(suggest_similar("activ", options), Some("active"));
/// assert_eq!(suggest_similar("xyz", options), None);
/// ```
pub fn suggest_similar<'a>(input: &str, valid_options: &[&'a str]) -> Option<&'a str> {
  let lower_input = input.trim().to_lowercase();
  if lower_input.is_empty() {
    return None;
  }

  let mut best_score = SUGGESTION_THRESHOLD;
  let mut best_option = None;

  for &option in valid_options {
    // Both directions: catches 'activ' -> 'active' AND 'actives' -> 'active'
    let score = score_haystack(&lower_input, option).max(score_haystack(option, &lower_input));

    if score > best_score {
      best_score = score;
      best_option = Some(option);
    }
  }

  best_option
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
  use super::*;

  fn make_window(id: &str, class: &str, proc: &str, visible: bool, managed: bool) -> FoundWindow {
    FoundWindow {
      id: id.to_string(),
      class_lowercase: class.to_lowercase(),
      proc_lowercase: proc.to_lowercase(),
      pid: 0,
      is_visible: visible,
      is_managed: managed,
    }
  }

  #[test]
  fn test_exact_match() {
    let candidates = vec![make_window("1", "wezterm", "wezterm", true, false)];
    let result = fuzzy_match_window("wezterm", &candidates);
    assert!(result.is_some());
    assert_eq!(result.unwrap().id, "1");
  }

  #[test]
  fn test_substring_match() {
    let candidates = vec![make_window("1", "org.wezfurlong.wezterm", "", true, false)];
    let result = fuzzy_match_window("wezterm", &candidates);
    assert!(result.is_some());
  }

  #[test]
  fn test_empty_target_returns_none() {
    let candidates = vec![make_window("1", "wezterm", "", true, false)];
    let result = fuzzy_match_window("", &candidates);
    assert!(result.is_none());
  }

  #[test]
  fn test_no_candidates_returns_none() {
    let result = fuzzy_match_window("wezterm", &[]);
    assert!(result.is_none());
  }

  #[test]
  fn test_visibility_bonus() {
    let candidates = vec![
      make_window("1", "wezterm", "", false, false),
      make_window("2", "wezterm", "", true, false),
    ];
    // Both match equally, but visible one should win
    let result = fuzzy_match_window("wezterm", &candidates);
    assert_eq!(result.unwrap().id, "2");
  }

  #[test]
  fn test_managed_bonus() {
    let candidates = vec![
      make_window("1", "wezterm", "", true, false),
      make_window("2", "wezterm", "", true, true),
    ];
    // Both match equally, but managed one should win
    let result = fuzzy_match_window("wezterm", &candidates);
    assert_eq!(result.unwrap().id, "2");
  }

  #[test]
  fn test_no_match_when_no_subsequence() {
    // "xyz" doesn't appear at all in "abcdef"
    let candidates = vec![make_window("1", "abcdef", "", true, false)];
    let result = fuzzy_match_window("xyz", &candidates);
    assert!(result.is_none());
  }

  #[test]
  fn test_case_insensitive() {
    let candidates = vec![make_window("1", "WezTerm", "", true, false)];
    let result = fuzzy_match_window("wezterm", &candidates);
    assert!(result.is_some());
  }

  #[test]
  fn test_proc_name_fallback() {
    let candidates = vec![make_window("1", "some-class", "wezterm-gui", true, false)];
    let result = fuzzy_match_window("wezterm", &candidates);
    assert!(result.is_some());
  }

  #[test]
  fn test_window_typo_matching() {
    // "weztrem" (typo) should match "wezterm" window if no better match exists
    let candidates = vec![make_window("1", "wezterm", "", true, false)];
    let result = fuzzy_match_window("weztrem", &candidates);
    assert!(result.is_some());
    assert_eq!(result.unwrap().id, "1");
  }

  #[test]
  fn test_suggest_similar_typo() {
    let options = vec!["help", "active", "follow-mouse"];
    // "hlep" is a typo for "help"
    assert_eq!(suggest_similar("hlep", &options), Some("help"));
    // "activ" is a substring/subsequence
    assert_eq!(suggest_similar("activ", &options), Some("active"));
  }
}
