use chrono::{DateTime, Duration, Local};
use gpui::{Pixels, px};

const REBASE_DISTANCE: f64 = 16_384.0;

#[derive(Clone, Copy)]
pub(super) struct TimelineGeometry {
    epoch: DateTime<Local>,
    pixel_duration: Duration,
    scale: f64,
    origin: f64,
    translation: f64,
}

// Ties must commute with integer translations, including across zero.
fn snap(value: f64) -> f64 {
    (value + 0.5).floor()
}

impl TimelineGeometry {
    pub(super) fn new(epoch: DateTime<Local>, pixel_duration: Duration, scale: f32) -> Self {
        assert!(pixel_duration > Duration::zero());
        assert!(scale.is_finite() && scale > 0.0);
        Self {
            epoch,
            pixel_duration,
            scale: f64::from(scale),
            origin: 0.0,
            translation: 0.0,
        }
    }

    pub(super) fn update(
        &mut self,
        now: DateTime<Local>,
        pixel_duration: Duration,
        scroll_offset: Pixels,
        center_y: Pixels,
        scale: f32,
    ) -> (f32, Pixels) {
        assert!(pixel_duration > Duration::zero());
        assert!(scale.is_finite() && scale > 0.0);
        let previous = *self;
        let scale = f64::from(scale);
        let zoom_changed = previous.pixel_duration != pixel_duration || previous.scale != scale;
        self.pixel_duration = pixel_duration;
        self.scale = scale;

        let now_y = self.world(now);
        let scroll_y = f64::from(scroll_offset.as_f32()) * scale;
        let viewport_origin = now_y - scroll_y;
        if zoom_changed || (viewport_origin - self.origin).abs() > REBASE_DISTANCE {
            // Integer device-pixel rebases preserve the local snapping phase.
            self.origin = snap(viewport_origin);
        }
        self.translation =
            snap(f64::from(center_y.as_f32()) * scale + scroll_y - now_y + self.origin);

        self.rebase_from(previous)
    }

    pub(super) fn rebase_from(&self, previous: Self) -> (f32, Pixels) {
        debug_assert_eq!(self.epoch, previous.epoch);
        let ratio = previous.pixel_duration.as_seconds_f64() / self.pixel_duration.as_seconds_f64();
        let offset = previous.origin / previous.scale * ratio - self.origin / self.scale;
        (ratio as f32, px(offset as f32))
    }

    pub(super) fn offset(&self, time: DateTime<Local>) -> Pixels {
        // Snap before narrowing so rebasing cannot change which side of a half-pixel a time lies on.
        px(((snap(self.world(time)) - self.origin) / self.scale) as f32)
    }

    pub(super) fn y(&self, offset: Pixels) -> Pixels {
        let local = snap(f64::from(offset.as_f32()) * self.scale);
        px(((local + self.translation) / self.scale) as f32)
    }

    pub(super) fn offset_at_y(&self, y: Pixels) -> Pixels {
        px(((f64::from(y.as_f32()) * self.scale - self.translation) / self.scale) as f32)
    }

    pub(super) fn time_at_offset(&self, offset: Pixels) -> DateTime<Local> {
        let seconds = (self.origin / self.scale + f64::from(offset.as_f32()))
            * self.pixel_duration.as_seconds_f64();
        let duration = Duration::seconds(seconds.trunc() as i64)
            + Duration::nanoseconds((seconds.fract() * 1_000_000_000.0).round() as i64);
        self.epoch + duration
    }

    pub(super) fn time_at_y(&self, y: Pixels) -> DateTime<Local> {
        self.time_at_offset(self.offset_at_y(y))
    }

    fn world(&self, time: DateTime<Local>) -> f64 {
        (time - self.epoch).as_seconds_f64() / self.pixel_duration.as_seconds_f64() * self.scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    const SCALES: [f32; 4] = [1.0, 1.25, 1.5, 2.0];

    fn epoch() -> DateTime<Local> {
        Utc.timestamp_opt(1_700_000_000, 0)
            .single()
            .unwrap()
            .with_timezone(&Local)
    }

    fn device_y(y: Pixels, scale: f32) -> i64 {
        let value = f64::from(y.as_f32()) * f64::from(scale);
        assert!((value - value.round()).abs() < 0.002);
        value.round() as i64
    }

    fn assert_near(actual: Pixels, expected: Pixels) {
        assert!(
            (actual - expected).as_f32().abs() < 0.001,
            "actual: {actual:?}, expected: {expected:?}"
        );
    }

    #[test]
    fn slow_scrolling_is_rigid_and_keeps_targets_stable() {
        let epoch = epoch();
        let duration = Duration::seconds(12);
        let times = [-36_001, -6_000, -1, 1, 6_000, 36_001, 1_234_567]
            .map(|ms| epoch + Duration::milliseconds(ms));
        for scale in SCALES {
            for direction in [-1.0, 1.0] {
                let mut geometry = TimelineGeometry::new(epoch, duration, scale);
                geometry.update(epoch, duration, px(0.0), px(300.25), scale);
                let offsets = times.map(|time| geometry.offset(time));
                let baseline = offsets.map(|offset| device_y(geometry.y(offset), scale));
                for frame in 1..=320 {
                    let now = epoch + Duration::milliseconds(frame * 40);
                    let scroll = px(direction * frame as f32 / 64.0);
                    let rebase = geometry.update(now, duration, scroll, px(300.25), scale);
                    assert_eq!(rebase, (1.0, px(0.0)));
                    assert_eq!(times.map(|time| geometry.offset(time)), offsets);
                    let positions = offsets.map(|offset| device_y(geometry.y(offset), scale));
                    let movement = positions[0] - baseline[0];
                    for (position, previous) in positions.into_iter().zip(baseline) {
                        assert_eq!(position - previous, movement);
                    }
                }
            }
        }
    }

    #[test]
    fn rebasing_preserves_screen_mapping_across_zero_and_half_ties() {
        let epoch = epoch();
        let duration = Duration::seconds(1);
        let times = [-1_500, -500, 500, 1_500, 19_999_500, 20_000_500]
            .map(|ms| epoch + Duration::milliseconds(ms));
        for scale in SCALES {
            let mut geometry = TimelineGeometry::new(epoch, duration, scale);
            geometry.update(epoch, duration, px(0.0), px(256.0), scale);
            let offsets = times.map(|time| geometry.offset(time));
            let baseline = offsets.map(|offset| device_y(geometry.y(offset), scale));
            if scale == 1.0 {
                assert_eq!(&baseline[..4], &[255, 256, 257, 258]);
            }

            let now = epoch + Duration::seconds(20_000);
            let (ratio, delta) = geometry.update(now, duration, px(0.0), px(256.0), scale);
            assert_eq!(ratio, 1.0);
            assert_eq!(delta, px(-20_000.0));
            let movement = -(20_000.0 * f64::from(scale)) as i64;
            for ((time, offset), previous) in times.into_iter().zip(offsets).zip(baseline) {
                let local = geometry.offset(time);
                assert_near(offset * ratio + delta, local);
                assert_eq!(device_y(geometry.y(local), scale) - previous, movement);
            }
            assert!(geometry.offset(now).as_f32().abs() < 1.0);
        }
    }

    #[test]
    fn near_half_time_points_keep_their_spacing_across_an_origin_rebase() {
        let epoch = epoch();
        let duration = Duration::seconds(1);
        let times = [16_384_499_900, 16_384_250_000, 16_385_500_100]
            .map(|us| epoch + Duration::microseconds(us));
        let mut geometry = TimelineGeometry::new(epoch, duration, 1.0);
        geometry.update(
            epoch + Duration::milliseconds(16_383_990),
            duration,
            px(0.0),
            px(300.0),
            1.0,
        );
        let previous = geometry;
        let offsets = times.map(|time| geometry.offset(time));
        let before = offsets.map(|offset| device_y(geometry.y(offset), 1.0));
        let (ratio, delta) = geometry.update(
            epoch + Duration::milliseconds(16_384_030),
            duration,
            px(0.0),
            px(300.0),
            1.0,
        );
        assert_ne!(geometry.origin, previous.origin);
        assert_eq!((ratio, delta), (1.0, px(-16_384.0)));
        let after = times.map(|time| device_y(geometry.y(geometry.offset(time)), 1.0));
        assert_eq!(before, [300, 300, 302]);
        assert_eq!(after, before);
        for (offset, shown) in offsets.into_iter().zip(after) {
            assert_eq!(device_y(geometry.y(offset * ratio + delta), 1.0), shown);
        }
    }

    #[test]
    fn zoom_and_scale_rebases_stay_within_old_and_new_grid_rounding() {
        let epoch = epoch();
        let now = epoch + Duration::days(600) + Duration::milliseconds(17);
        let times = [-10_001, -1, 1, 20_003].map(|ms| now + Duration::milliseconds(ms));
        let mut geometry = TimelineGeometry::new(epoch, Duration::seconds(12), 1.25);
        geometry.update(now, Duration::seconds(12), px(17.25), px(300.25), 1.25);
        let initial = geometry;
        for (duration, scale) in [(Duration::seconds(5), 2.0), (Duration::seconds(5), 1.5)] {
            let previous = geometry;
            let rebase = geometry.update(now, duration, px(17.25), px(300.25), scale);
            assert_eq!(rebase, geometry.rebase_from(previous));
            for retained in [previous, initial] {
                let (ratio, delta) = geometry.rebase_from(retained);
                let rounding_bound = 0.5 / retained.scale
                    * retained.pixel_duration.as_seconds_f64()
                    / duration.as_seconds_f64()
                    + 0.5 / geometry.scale;
                for time in times {
                    let rebased = retained.offset(time) * ratio + delta;
                    let fresh = geometry.offset(time);
                    let error = f64::from((rebased - fresh).as_f32().abs());
                    assert!(error <= rounding_bound + 0.001);
                }
            }
            assert_eq!(geometry.rebase_from(geometry), (1.0, px(0.0)));
            assert_eq!(
                geometry.update(now, duration, px(17.25), px(300.25), scale),
                (1.0, px(0.0))
            );
        }
    }

    #[test]
    fn inverse_mapping_uses_the_displayed_translation() {
        let epoch = epoch();
        let duration = Duration::seconds(12);
        let times = [-13_499, -1, 1, 17_003].map(|ms| epoch + Duration::milliseconds(ms));
        for scale in SCALES {
            let mut geometry = TimelineGeometry::new(epoch, duration, scale);
            for frame in 0..80 {
                let now = epoch + Duration::milliseconds(70_000 + frame * 40);
                geometry.update(now, duration, px(5.25), px(240.25), scale);
                for time in times {
                    let offset = geometry.offset(time);
                    let shown = geometry.y(offset);
                    let inverse = geometry.offset_at_y(shown);
                    assert_near(inverse, offset);
                    assert_eq!(geometry.y(inverse), shown);
                    let rounding_bound = duration.as_seconds_f64() / (2.0 * f64::from(scale));
                    for recovered in [geometry.time_at_y(shown), geometry.time_at_offset(offset)] {
                        let error = (recovered - time).as_seconds_f64().abs();
                        assert!(error <= rounding_bound + 0.0001);
                    }
                }
            }
        }
    }

    #[test]
    fn long_navigation_keeps_local_precision_and_time_range() {
        let epoch = epoch();
        let duration = Duration::milliseconds(50);
        for scale in SCALES {
            for direction in [-1, 1] {
                let now = epoch + Duration::days(direction * 365_000) + Duration::milliseconds(17);
                let mut geometry = TimelineGeometry::new(epoch, duration, scale);
                geometry.update(now, duration, px(0.0), px(383.25), scale);
                assert!(geometry.offset(now).as_f32().abs() <= 0.5 / scale + 0.001);
                let time = now + Duration::milliseconds(125);
                let next = time + duration * 4;
                assert_near(geometry.offset(next) - geometry.offset(time), px(4.0));
                let shown = geometry.y(geometry.offset(time));
                let rounding_bound = duration.as_seconds_f64() / (2.0 * f64::from(scale));
                for recovered in [
                    geometry.time_at_offset(geometry.offset(time)),
                    geometry.time_at_y(shown),
                ] {
                    let error = (recovered - time).as_seconds_f64().abs();
                    assert!(error <= rounding_bound + 0.0001);
                }
                device_y(shown, scale);
            }
        }
    }
}
