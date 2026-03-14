use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

fn elapsed_since_start() -> Duration {
    PROCESS_START.get_or_init(Instant::now).elapsed()
}

fn has_true_color() -> bool {
    static TRUE_COLOR: OnceLock<bool> = OnceLock::new();
    *TRUE_COLOR.get_or_init(|| std::env::var("COLORTERM").map(|v| v == "truecolor" || v == "24bit").unwrap_or(false))
}

fn blend(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let r = (a.0 as f32 * t + b.0 as f32 * (1.0 - t)) as u8;
    let g = (a.1 as f32 * t + b.1 as f32 * (1.0 - t)) as u8;
    let b_val = (a.2 as f32 * t + b.2 as f32 * (1.0 - t)) as u8;
    (r, g, b_val)
}

/// Shimmer animation: a bright band sweeps across text on a 2-second cycle.
///
/// For multi-segment use (e.g. table rows), create one `Shimmer` with the total
/// width and call `spans()` per segment with its column offset. For single-text
/// use, call `shimmer_spans()` which wraps this with offset 0.
pub(crate) struct Shimmer {
    pos: f32,
    band_half_width: f32,
    true_color: bool,
    padding: usize,
}

impl Shimmer {
    pub fn new(total_width: usize) -> Self {
        Self::new_at(total_width, elapsed_since_start())
    }

    pub fn new_at(total_width: usize, elapsed: Duration) -> Self {
        let padding = 10usize;
        let period = total_width + padding * 2;
        let sweep_seconds = 2.0f32;
        let pos = (elapsed.as_secs_f32() % sweep_seconds) / sweep_seconds * period as f32;
        Self { pos, band_half_width: 5.0, true_color: has_true_color(), padding }
    }

    /// Render a segment of the shimmer at `offset` characters from the row start.
    pub fn spans(&self, text: &str, offset: usize) -> Vec<Span<'static>> {
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return Vec::new();
        }

        let base: (u8, u8, u8) = (140, 130, 40);
        let highlight: (u8, u8, u8) = (255, 240, 120);

        let mut spans = Vec::with_capacity(chars.len());
        for (i, ch) in chars.iter().enumerate() {
            let dist = (((offset + i) as f32 + self.padding as f32) - self.pos).abs();
            let t =
                if dist <= self.band_half_width { 0.5 * (1.0 + (std::f32::consts::PI * dist / self.band_half_width).cos()) } else { 0.0 };

            let style = if self.true_color {
                let (r, g, b) = blend(highlight, base, t);
                Style::default().fg(Color::Rgb(r, g, b))
            } else if t < 0.2 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::DIM)
            } else if t < 0.6 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            };

            spans.push(Span::styled(ch.to_string(), style));
        }
        spans
    }
}

/// Convenience wrapper -- single-segment shimmer (status bar, etc.).
pub(crate) fn shimmer_spans(text: &str) -> Vec<Span<'static>> {
    Shimmer::new(text.chars().count()).spans(text, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shimmer_spans_wrapper_matches_struct() {
        let text = "hello world";
        let expected = shimmer_spans(text);
        let shimmer = Shimmer::new(text.chars().count());
        let actual = shimmer.spans(text, 0);
        assert_eq!(expected.len(), actual.len());
        for (e, a) in expected.iter().zip(actual.iter()) {
            assert_eq!(e.style, a.style);
            assert_eq!(e.content, a.content);
        }
    }

    #[test]
    fn new_at_deterministic() {
        let elapsed = Duration::from_millis(500);
        let s1 = Shimmer::new_at(20, elapsed);
        let s2 = Shimmer::new_at(20, elapsed);
        let spans1 = s1.spans("test", 0);
        let spans2 = s2.spans("test", 0);
        for (a, b) in spans1.iter().zip(spans2.iter()) {
            assert_eq!(a.style, b.style);
        }
    }

    #[test]
    fn offset_shifts_band_position() {
        let elapsed = Duration::from_millis(500);
        let shimmer = Shimmer::new_at(40, elapsed);
        let at_zero = shimmer.spans("ab", 0);
        let at_twenty = shimmer.spans("ab", 20);
        let styles_differ = at_zero.iter().zip(at_twenty.iter()).any(|(a, b)| a.style != b.style);
        assert!(styles_differ, "offset should shift the shimmer band");
    }

    #[test]
    fn empty_text_returns_empty_spans() {
        let shimmer = Shimmer::new(10);
        assert!(shimmer.spans("", 0).is_empty());
    }
}
