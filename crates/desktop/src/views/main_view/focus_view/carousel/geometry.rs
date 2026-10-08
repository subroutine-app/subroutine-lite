use gpui::px;

use crate::settings::FocusCarouselOrientation;

const CAROUSEL_CARD_HEIGHT: gpui::Pixels = px(206.);
const CAROUSEL_CARD_WIDTH: f32 = 0.52;
const CAROUSEL_HORIZONTAL_HEIGHT_SCALE: f32 = 1.3;
const CAROUSEL_VERTICAL_WIDTH_SCALE: f32 = 1.45;
const CAROUSEL_CARD_SCALE: f32 = 1.5;
pub(super) const CAROUSEL_GAP_PX: f32 = 64.0;

const CAROUSEL_ANGLE_PER_ITEM: f32 = 0.82;

#[derive(Clone, Copy, Debug)]
pub(super) struct CarouselGeometry {
    pub(super) center: f32,
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) scale: f32,
    pub(super) opacity: f32,
}

pub(super) const MIN_CAROUSEL_SCALE: f32 = 0.34;
const CAROUSEL_SCALE_RANGE: f32 = 1.0 - MIN_CAROUSEL_SCALE;

fn sampled_carousel_scale(index: usize) -> f32 {
    let angle = (index as f32 * CAROUSEL_ANGLE_PER_ITEM).min(std::f32::consts::FRAC_PI_2);
    MIN_CAROUSEL_SCALE + CAROUSEL_SCALE_RANGE * angle.cos().max(0.0)
}

fn carousel_segment(distance: f32) -> (usize, f32) {
    let distance = distance.abs();
    let index = distance.floor() as usize;
    let fraction = distance - index as f32;
    let phase = (1.0 - (std::f32::consts::PI * fraction).cos()) / 2.0;
    (index, phase)
}

fn carousel_spacing(index: usize, primary_size: f32) -> f32 {
    primary_size * (sampled_carousel_scale(index) + sampled_carousel_scale(index + 1)) / 2.0
        + CAROUSEL_GAP_PX
}

fn carousel_offset(distance: f32, primary_size: f32) -> f32 {
    let (index, phase) = carousel_segment(distance);
    let curved_segments = (std::f32::consts::FRAC_PI_2 / CAROUSEL_ANGLE_PER_ITEM).ceil() as usize;
    let explicit = index.min(curved_segments);
    let mut offset = (0..explicit)
        .map(|segment| carousel_spacing(segment, primary_size))
        .sum::<f32>();
    if index > curved_segments {
        offset += (index - curved_segments) as f32
            * (primary_size * MIN_CAROUSEL_SCALE + CAROUSEL_GAP_PX);
    }
    offset + phase * carousel_spacing(index, primary_size)
}

pub(super) fn carousel_base_size(
    viewport_width: f32,
    viewport_height: f32,
    orientation: FocusCarouselOrientation,
) -> (f32, f32) {
    let width = (viewport_width * CAROUSEL_CARD_WIDTH).clamp(1.0, 360.0);
    let height = (width / 1.75).min(f32::from(CAROUSEL_CARD_HEIGHT));
    let available_width = (viewport_width - 8.0).max(1.0);
    let available_height = (viewport_height - 8.0).max(1.0);
    match orientation {
        FocusCarouselOrientation::Horizontal => (
            (width * CAROUSEL_CARD_SCALE).min(available_width),
            (height * CAROUSEL_HORIZONTAL_HEIGHT_SCALE * CAROUSEL_CARD_SCALE).min(available_height),
        ),
        FocusCarouselOrientation::Vertical => (
            (width * CAROUSEL_VERTICAL_WIDTH_SCALE * CAROUSEL_CARD_SCALE).min(available_width),
            (height * CAROUSEL_CARD_SCALE).min(available_height),
        ),
    }
}

pub(super) fn carousel_geometry(
    distance: f32,
    base_width: f32,
    base_height: f32,
    orientation: FocusCarouselOrientation,
    viewport_extent: f32,
) -> CarouselGeometry {
    let (index, phase) = carousel_segment(distance);
    let start_scale = sampled_carousel_scale(index);
    let end_scale = sampled_carousel_scale(index + 1);
    let scale = start_scale + (end_scale - start_scale) * phase;
    let primary_size = match orientation {
        FocusCarouselOrientation::Horizontal => base_width,
        FocusCarouselOrientation::Vertical => base_height,
    };
    let offset = carousel_offset(distance, primary_size);
    CarouselGeometry {
        center: 0.5 + distance.signum() * offset / viewport_extent.max(1.0),
        width: base_width * scale,
        height: base_height * scale,
        scale,
        opacity: 0.12 + 0.88 * ((scale - MIN_CAROUSEL_SCALE) / CAROUSEL_SCALE_RANGE),
    }
}

pub(super) fn vertical_centers(extents: &[(f32, f32)]) -> Vec<f32> {
    let Some(&(first, _)) = extents.first() else {
        return Vec::new();
    };
    let mut centers = Vec::with_capacity(extents.len());
    centers.push(first);
    for (index, pair) in extents.windows(2).enumerate() {
        let spacing = (pair[0].1 + pair[1].1) / 2.0 + CAROUSEL_GAP_PX;
        centers.push(pair[1].0.max(centers[index] + spacing));
    }

    let right = extents.partition_point(|(center, _)| *center < 0.0);
    let correction = match right {
        0 => centers[0] - extents[0].0,
        index if index == extents.len() => centers[index - 1] - extents[index - 1].0,
        index => {
            let left = extents[index - 1].0;
            let right = extents[index].0;
            let phase = -left / (right - left);
            let start = centers[index - 1] - left;
            let end = centers[index] - right;
            start + (end - start) * phase
        }
    };
    centers
        .into_iter()
        .map(|center| center - correction)
        .collect()
}

pub(super) fn entrance_distance(target: f32) -> f32 {
    target + if target < 0.0 { -0.7 } else { 0.7 }
}
