use std::time::{Duration, Instant};

use gpui::Pixels;

const DWELL: Duration = Duration::from_millis(300);

/// Hover-zone autoscroll that only begins once the pointer has rested in an
/// edge zone for [`DWELL`], so working near an edge never scrolls by accident.
///
/// Speeds are signed: positive scrolls toward the top edge, negative toward
/// the bottom. Moving to the opposite edge restarts the dwell.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeScroll(Option<Hover>);

#[derive(Clone, Copy, Debug, PartialEq)]
struct Hover {
    speed: Pixels,
    since: Instant,
}

fn is_bottom(speed: Pixels) -> bool {
    f32::from(speed).is_sign_negative()
}

impl EdgeScroll {
    /// Returns whether the hover state changed.
    pub fn hover(&mut self, speed: Option<Pixels>) -> bool {
        self.hover_at(speed, Instant::now())
    }

    fn hover_at(&mut self, speed: Option<Pixels>, now: Instant) -> bool {
        let next = speed.map(|speed| Hover {
            speed,
            since: self
                .0
                .filter(|hover| is_bottom(hover.speed) == is_bottom(speed))
                .map_or(now, |hover| hover.since),
        });
        std::mem::replace(&mut self.0, next) != next
    }

    pub fn clear(&mut self) -> bool {
        self.0.take().is_some()
    }

    pub fn is_active(&self) -> bool {
        self.0.is_some()
    }

    pub fn is_waiting(&self) -> bool {
        self.is_active() && self.speed().is_none()
    }

    pub fn speed(&self) -> Option<Pixels> {
        self.speed_at(Instant::now())
    }

    fn speed_at(&self, now: Instant) -> Option<Pixels> {
        self.0
            .filter(|hover| now.saturating_duration_since(hover.since) >= DWELL)
            .map(|hover| hover.speed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    #[test]
    fn scrolls_only_after_dwell_on_one_edge() {
        let start = Instant::now();
        let mut edge = EdgeScroll::default();

        edge.hover_at(Some(px(4.)), start);
        assert_eq!(edge.speed_at(start + DWELL / 2), None);

        edge.hover_at(Some(px(9.)), start + DWELL / 2);
        assert_eq!(edge.speed_at(start + DWELL), Some(px(9.)));

        edge.hover_at(Some(px(-0.)), start + DWELL);
        assert_eq!(edge.speed_at(start + DWELL * 3 / 2), None);

        edge.hover_at(None, start + DWELL * 2);
        edge.hover_at(Some(px(-3.)), start + DWELL * 2);
        assert_eq!(edge.speed_at(start + DWELL * 5 / 2), None);
        assert_eq!(edge.speed_at(start + DWELL * 3), Some(px(-3.)));
    }
}
