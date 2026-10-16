use std::{cell::RefCell, ops::Range, rc::Rc};

use gpui::{App, Bounds, Pixels, ScrollHandle, Window, point, px};
use gpui_kit::motion::Transition;

use super::transition::SCROLL;

pub(crate) fn scroll_ranges(visible: Range<usize>, destination: Range<usize>) -> Vec<Range<usize>> {
    let (first, second) = if visible.start <= destination.start {
        (visible, destination)
    } else {
        (destination, visible)
    };
    if first.end >= second.start {
        vec![first.start..first.end.max(second.end)]
    } else {
        vec![first, second]
    }
}

#[derive(Clone, Default)]
pub(crate) struct ScrollMotion(Rc<RefCell<Option<Transition<Pixels>>>>);

impl ScrollMotion {
    pub(crate) fn cancel(&self) {
        *self.0.borrow_mut() = None;
    }

    pub(crate) fn target(&self, scroll: &ScrollHandle) -> Pixels {
        self.transition(scroll)
            .map_or_else(|| scroll.offset().y, |transition| transition.target())
    }

    pub(crate) fn is_animating(&self, scroll: &ScrollHandle) -> bool {
        self.transition(scroll)
            .is_some_and(|transition| transition.is_animating())
    }

    pub(crate) fn set(&self, scroll: &ScrollHandle, target: Pixels) {
        let mut transition = self
            .transition(scroll)
            .unwrap_or_else(|| Transition::new(scroll.offset().y, SCROLL));
        if target == scroll.offset().y {
            self.cancel();
            return;
        }
        transition.set(target);
        *self.0.borrow_mut() = Some(transition);
    }

    pub(crate) fn animate(
        &self,
        scroll: &ScrollHandle,
        max_scroll: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(mut transition) = self.clamped(scroll, max_scroll) {
            transition.animate(window, cx);
            self.write(scroll, transition);
        }
    }

    pub(crate) fn rebase(&self, scroll: &ScrollHandle, next: Pixels) {
        if let Some(mut transition) = self.transition(scroll) {
            transition.offset_by(next - scroll.offset().y);
            // Write the recomputed sample so rounding cannot look like external input.
            self.write(scroll, transition);
        } else {
            scroll.set_offset(point(scroll.offset().x, next));
        }
    }

    pub(crate) fn reveal(
        &self,
        scroll: &ScrollHandle,
        bounds: Bounds<Pixels>,
        viewport: Bounds<Pixels>,
        inset: Pixels,
    ) -> bool {
        let start = viewport.top() + inset.max(px(0.));
        let end = viewport.bottom();
        let available = end - start;
        if available <= px(0.) {
            return false;
        }

        let target = self.target(scroll);
        let top = bounds.top() + (target - scroll.offset().y);
        let bottom = top + bounds.size.height.min(available);
        let next = if top < start {
            target + (start - top)
        } else if bottom > end {
            target - (bottom - end)
        } else {
            return false;
        };
        if next == target {
            return false;
        }
        self.set(scroll, next);
        true
    }

    fn transition(&self, scroll: &ScrollHandle) -> Option<Transition<Pixels>> {
        let mut transition = self.0.borrow_mut();
        if transition.is_some_and(|transition| transition.value() != scroll.offset().y) {
            *transition = None;
        }
        *transition
    }

    fn clamped(&self, scroll: &ScrollHandle, max_scroll: Pixels) -> Option<Transition<Pixels>> {
        let min = -max_scroll.max(px(0.));
        let target = self.target(scroll).clamp(min, px(0.));
        let offset = scroll.offset();
        let current = offset.y.clamp(min, px(0.));
        if current != offset.y {
            self.cancel();
            scroll.set_offset(point(offset.x, current));
        }
        self.set(scroll, target);
        *self.0.borrow()
    }

    fn write(&self, scroll: &ScrollHandle, transition: Transition<Pixels>) {
        let animating = transition.is_animating();
        let value = if animating {
            transition.value()
        } else {
            transition.target()
        };
        scroll.set_offset(point(scroll.offset().x, value));
        *self.0.borrow_mut() = animating.then_some(transition);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui::size;

    use super::*;

    fn advance(motion: &ScrollMotion, scroll: &ScrollHandle, max: Pixels, millis: u64) {
        if let Some(mut transition) = motion.clamped(scroll, max) {
            transition.advance(Duration::from_millis(millis));
            motion.write(scroll, transition);
        }
    }

    #[test]
    fn scroll_ranges_mount_destination_without_rendering_the_gap() {
        assert_eq!(scroll_ranges(0..10, 90..100), vec![0..10, 90..100]);
        assert_eq!(scroll_ranges(90..100, 0..10), vec![0..10, 90..100]);
        assert_eq!(scroll_ranges(0..10, 5..15), vec![0..15]);
        assert_eq!(scroll_ranges(0..10, 10..20), vec![0..20]);
    }

    #[test]
    fn external_vertical_input_cancels_shared_motion() {
        let motion = ScrollMotion::default();
        let shared = motion.clone();
        let scroll = ScrollHandle::new();
        motion.set(&scroll, px(-200.));
        advance(&motion, &scroll, px(500.), 50);

        scroll.set_offset(point(px(-12.), scroll.offset().y));
        assert!(shared.is_animating(&scroll));
        scroll.set_offset(point(px(-12.), px(-25.)));
        assert_eq!(shared.target(&scroll), px(-25.));
        assert!(!motion.is_animating(&scroll));
        advance(&motion, &scroll, px(500.), 200);
        assert_eq!(scroll.offset(), point(px(-12.), px(-25.)));

        motion.set(&scroll, px(-300.));
        scroll.set_offset(point(px(-12.), px(-40.)));
        assert!(!shared.is_animating(&scroll));
        motion.set(&scroll, px(-100.));
        assert_eq!(motion.0.borrow().unwrap().value(), px(-40.));
        shared.cancel();
        assert!(!motion.is_animating(&scroll));
        assert_eq!(motion.target(&scroll), px(-40.));
    }

    #[test]
    fn retargets_from_displayed_offset_without_restarting_repeated_targets() {
        let motion = ScrollMotion::default();
        let scroll = ScrollHandle::new();
        motion.set(&scroll, px(-200.));
        advance(&motion, &scroll, px(500.), 80);
        let displayed = scroll.offset().y;
        assert!(displayed < px(0.) && displayed > px(-200.));
        let previous = motion.0.borrow().unwrap();
        motion.set(&scroll, px(-200.));
        assert_eq!(motion.0.borrow().unwrap(), previous);

        motion.set(&scroll, px(-50.));
        assert_eq!(scroll.offset().y, displayed);
        assert_eq!(motion.0.borrow().unwrap().value(), displayed);
        advance(&motion, &scroll, px(500.), 80);
        motion.set(&scroll, px(-50.));
        advance(&motion, &scroll, px(500.), 120);
        assert_eq!(scroll.offset().y, px(-50.));
        assert!(!motion.is_animating(&scroll));

        motion.set(&scroll, px(-300.));
        motion.set(&scroll, scroll.offset().y);
        assert!(!motion.is_animating(&scroll));
    }

    #[test]
    fn anchor_rebase_preserves_remaining_travel_and_playhead() {
        let motion = ScrollMotion::default();
        let scroll = ScrollHandle::new();
        scroll.set_offset(point(px(-7.), px(-19.3)));
        motion.set(&scroll, px(-283.7));
        advance(&motion, &scroll, px(500.), 73);
        let mut expected = motion.0.borrow().unwrap();
        let next = scroll.offset().y + px(31.9);
        let delta = next - scroll.offset().y;
        expected.offset_by(delta);

        motion.rebase(&scroll, next);
        assert!((scroll.offset().y - next).abs() < px(0.001));
        assert!(motion.is_animating(&scroll));
        assert_eq!(motion.0.borrow().unwrap(), expected);
        advance(&motion, &scroll, px(500.), 127);
        assert_eq!(scroll.offset(), point(px(-7.), expected.target()));
        assert!(!motion.is_animating(&scroll));

        motion.rebase(&scroll, px(-10.));
        assert_eq!(scroll.offset(), point(px(-7.), px(-10.)));
        assert!(!motion.is_animating(&scroll));
    }

    #[test]
    fn reveal_checks_destination_without_writing_the_offset() {
        let motion = ScrollMotion::default();
        let scroll = ScrollHandle::new();
        let viewport = Bounds::new(point(px(0.), px(100.)), size(px(200.), px(200.)));
        let row = Bounds::new(point(px(0.), px(350.)), size(px(100.), px(20.)));
        motion.set(&scroll, px(-100.));
        assert!(!motion.reveal(&scroll, row, viewport, px(20.)));
        assert_eq!(motion.target(&scroll), px(-100.));

        let row = Bounds::new(point(px(0.), px(150.)), row.size);
        assert!(motion.reveal(&scroll, row, viewport, px(20.)));
        assert_eq!(motion.target(&scroll), px(-30.));
        assert_eq!(scroll.offset().y, px(0.));
        assert!(!motion.reveal(&scroll, row, viewport, px(20.)));

        advance(&motion, &scroll, px(500.), 80);
        let row = Bounds::new(point(px(0.), px(150.) + scroll.offset().y), row.size);
        let previous = motion.0.borrow().unwrap();
        assert!(!motion.reveal(&scroll, row, viewport, px(20.)));
        assert_eq!(motion.0.borrow().unwrap(), previous);
    }

    #[test]
    fn oversized_reveal_aligns_top_and_empty_viewports_do_not_queue() {
        let motion = ScrollMotion::default();
        let scroll = ScrollHandle::new();
        let viewport = Bounds::new(point(px(0.), px(100.)), size(px(200.), px(200.)));
        let row = Bounds::new(point(px(0.), px(350.)), size(px(100.), px(400.)));
        assert!(!motion.reveal(&scroll, row, viewport, px(200.)));
        assert!(!motion.is_animating(&scroll));
        assert!(motion.reveal(&scroll, row, viewport, px(20.)));
        assert_eq!(motion.target(&scroll), px(-230.));
        assert_eq!(scroll.offset().y, px(0.));
        assert!(!motion.reveal(&scroll, row, viewport, px(20.)));
    }

    #[test]
    fn clamps_shrinking_ranges_and_writes_the_final_sample() {
        let motion = ScrollMotion::default();
        let scroll = ScrollHandle::new();
        scroll.set_offset(point(px(-8.), px(-20.)));
        motion.set(&scroll, px(-300.));
        advance(&motion, &scroll, px(100.), 50);
        assert_eq!(motion.target(&scroll), px(-100.));
        assert!(scroll.offset().y >= px(-100.) && scroll.offset().y <= px(0.));
        advance(&motion, &scroll, px(100.), 150);
        assert_eq!(scroll.offset(), point(px(-8.), px(-100.)));
        assert!(!motion.is_animating(&scroll));

        motion.set(&scroll, px(-20.));
        advance(&motion, &scroll, px(50.), 0);
        assert_eq!(scroll.offset().y, px(-50.));
        assert_eq!(motion.target(&scroll), px(-20.));
        advance(&motion, &scroll, px(50.), 200);
        assert_eq!(scroll.offset().y, px(-20.));

        motion.set(&scroll, px(200.));
        advance(&motion, &scroll, px(-10.), 200);
        assert_eq!(scroll.offset(), point(px(-8.), px(0.)));
        assert!(!motion.is_animating(&scroll));
    }
}
