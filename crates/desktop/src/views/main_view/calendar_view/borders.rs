use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use gpui::{App, Bounds, Hsla, IntoElement, Pixels, Styled, canvas, fill, point, px, size};
use gpui_kit_theme::ActiveTheme;

use crate::{color::ColorExt, dates::InclusiveDateRange};

#[derive(Clone, Copy, Debug, PartialEq)]
enum BorderStyle {
    Grid,
    Month(f32),
    Selected,
}

impl BorderStyle {
    fn width(self, scale: f32) -> i32 {
        let width = match self {
            Self::Month(progress) => 1.0 + 0.5 * progress,
            Self::Grid | Self::Selected => 1.0,
        };
        (width * scale).round().max(1.0) as i32
    }

    fn priority(self) -> f32 {
        match self {
            Self::Grid => 0.0,
            Self::Month(progress) => 1.0 + progress,
            Self::Selected => 3.0,
        }
    }

    fn color(self, colors: &BorderColors) -> Hsla {
        match self {
            Self::Grid => colors.grid,
            Self::Month(progress) => colors.month.mix(colors.grid, progress),
            Self::Selected => colors.selected,
        }
    }
}

struct BorderColors {
    grid: Hsla,
    month: Hsla,
    selected: Hsla,
}

pub(super) struct GridBorders {
    horizontal: Vec<[BorderStyle; 7]>,
    vertical: Vec<[BorderStyle; 8]>,
}

impl GridBorders {
    pub(super) fn new(
        start: NaiveDate,
        weeks: usize,
        selection: Option<InclusiveDateRange>,
        drop_target: Option<NaiveDate>,
        mut month_progress: impl FnMut(NaiveDate) -> f32,
    ) -> Self {
        let mut months = BTreeMap::new();
        let mut style_between = |before: Option<NaiveDate>, after: Option<NaiveDate>| {
            let month = match (before, after) {
                (Some(before), Some(after)) if before.with_day(1) != after.with_day(1) => {
                    let boundary = after.with_day(1).unwrap();
                    let progress = *months
                        .entry(boundary)
                        .or_insert_with(|| month_progress(boundary));
                    if progress > 0.0 {
                        BorderStyle::Month(progress)
                    } else {
                        BorderStyle::Grid
                    }
                }
                _ => BorderStyle::Grid,
            };
            if [before, after].into_iter().flatten().any(|date| {
                selection.is_some_and(|range| range.contains(date)) || drop_target == Some(date)
            }) {
                BorderStyle::Selected
            } else {
                month
            }
        };
        let date_at = |row, column| start + chrono::Duration::days((row * 7 + column) as i64);
        let horizontal = (0..=weeks)
            .map(|row| {
                std::array::from_fn(|column| {
                    let below = date_at(row, column);
                    style_between(Some(below - chrono::Duration::days(7)), Some(below))
                })
            })
            .collect();
        let vertical = (0..weeks)
            .map(|row| {
                std::array::from_fn(|column| {
                    let right = date_at(row, column);
                    style_between(
                        (column > 0).then(|| right - chrono::Duration::days(1)),
                        (column < 7).then_some(right),
                    )
                })
            })
            .collect();
        Self {
            horizontal,
            vertical,
        }
    }

    pub(super) fn render(
        self,
        first_row_top: Pixels,
        week_height: Pixels,
        sidebar_right: Option<Pixels>,
        cx: &App,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let colors = BorderColors {
            grid: theme.colors.hairline,
            month: theme
                .colors
                .hairline_strong
                .mix(theme.colors.hairline, 0.25),
            selected: theme.colors.focus,
        };
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let scale = window.scale_factor();
                for quad in self.quads(bounds, first_row_top, week_height, scale, sidebar_right) {
                    let bounds = quad.bounds;
                    window.paint_quad(fill(
                        Bounds::new(
                            point(
                                px(bounds.left() as f32 / scale),
                                px(bounds.top() as f32 / scale),
                            ),
                            size(
                                px(bounds.size.width as f32 / scale),
                                px(bounds.size.height as f32 / scale),
                            ),
                        ),
                        quad.style.color(&colors),
                    ));
                }
            },
        )
        .absolute()
        .inset_0()
    }

    fn quads(
        &self,
        bounds: Bounds<Pixels>,
        first_row_top: Pixels,
        week_height: Pixels,
        scale: f32,
        sidebar_right: Option<Pixels>,
    ) -> Vec<BorderQuad> {
        if self.vertical.is_empty() || bounds.size.width <= px(0.) {
            return Vec::new();
        }
        let left = (bounds.left().as_f32() * scale).ceil() as i32;
        let right = (bounds.right().as_f32() * scale).floor() as i32;
        let shared_left =
            sidebar_right.is_some_and(|edge| (edge.as_f32() * scale).ceil() as i32 == left);
        let x_band = |column, width| match column {
            0 => (left, left + width),
            7 => (right - width, right),
            _ => stroke_band(
                (bounds.left() + bounds.size.width * (column as f32 / 7.)).as_f32() * scale,
                width,
            ),
        };
        let y_band = |row, width| {
            stroke_band(
                (bounds.top() + first_row_top + week_height * row as f32).as_f32() * scale,
                width,
            )
        };
        let mut strokes = Vec::new();
        for (row, styles) in self.horizontal.iter().enumerate() {
            for (column, &style) in styles.iter().enumerate() {
                let width = style.width(scale);
                let (top, bottom) = y_band(row, width);
                strokes.push(BorderQuad::new(
                    x_band(column, width).0,
                    top,
                    x_band(column + 1, width).1,
                    bottom,
                    style,
                ));
            }
        }
        for (row, styles) in self.vertical.iter().enumerate() {
            for (column, &style) in styles.iter().enumerate() {
                if column == 0 && shared_left && style != BorderStyle::Selected {
                    continue;
                }
                let width = style.width(scale);
                let (left, right) = x_band(column, width);
                strokes.push(BorderQuad::new(
                    left,
                    y_band(row, width).0,
                    right,
                    y_band(row + 1, width).1,
                    style,
                ));
            }
        }
        merge_strokes(strokes)
    }
}

fn stroke_band(center: f32, width: i32) -> (i32, i32) {
    let start = (center - width as f32 / 2.).round() as i32;
    (start, start + width)
}

// Half-open device-pixel rectangles keep adjoining translucent strokes from blending twice.
#[derive(Clone, Copy, Debug)]
struct BorderQuad {
    bounds: Bounds<i32>,
    style: BorderStyle,
}

impl BorderQuad {
    fn new(left: i32, top: i32, right: i32, bottom: i32, style: BorderStyle) -> Self {
        Self {
            bounds: Bounds::new(point(left, top), size(right - left, bottom - top)),
            style,
        }
    }
}

fn merge_strokes(mut strokes: Vec<BorderQuad>) -> Vec<BorderQuad> {
    strokes.sort_by_key(|stroke| stroke.bounds.top());
    let mut ys: Vec<_> = strokes
        .iter()
        .flat_map(|stroke| [stroke.bounds.top(), stroke.bounds.bottom()])
        .collect();
    ys.sort_unstable();
    ys.dedup();

    let mut pending = strokes.iter().peekable();
    let mut active: Vec<&BorderQuad> = Vec::new();
    let mut quads: Vec<BorderQuad> = Vec::new();
    for band in ys.windows(2) {
        let (top, bottom) = (band[0], band[1]);
        active.retain(|stroke| stroke.bounds.bottom() > top);
        while pending
            .peek()
            .is_some_and(|stroke| stroke.bounds.top() <= top)
        {
            active.push(pending.next().unwrap());
        }
        let mut xs: Vec<_> = active
            .iter()
            .flat_map(|stroke| [stroke.bounds.left(), stroke.bounds.right()])
            .collect();
        xs.sort_unstable();
        xs.dedup();
        for span in xs.windows(2) {
            let (left, right) = (span[0], span[1]);
            let style = active
                .iter()
                .filter(|stroke| stroke.bounds.left() <= left && stroke.bounds.right() >= right)
                .map(|stroke| stroke.style)
                .max_by(|a, b| a.priority().total_cmp(&b.priority()));
            let Some(style) = style else { continue };
            if let Some(last) = quads.last_mut().filter(|last| {
                last.style == style && last.bounds.top() == top && last.bounds.right() == left
            }) {
                last.bounds.size.width = right - last.bounds.left();
            } else {
                quads.push(BorderQuad::new(left, top, right, bottom, style));
            }
        }
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(quad: &BorderQuad) -> impl Iterator<Item = (i32, i32)> {
        let bounds = quad.bounds;
        (bounds.top()..bounds.bottom())
            .flat_map(move |y| (bounds.left()..bounds.right()).map(move |x| (x, y)))
    }

    fn rasterize(quads: &[BorderQuad]) -> BTreeMap<(i32, i32), BorderStyle> {
        let mut raster = BTreeMap::new();
        for quad in quads {
            for pixel in pixels(quad) {
                assert!(
                    raster.insert(pixel, quad.style).is_none(),
                    "overdraw at {pixel:?}"
                );
            }
        }
        raster
    }

    #[test]
    fn intersections_preserve_coverage_and_style_without_overdraw() {
        let strokes = vec![
            BorderQuad::new(0, 3, 19, 5, BorderStyle::Grid),
            BorderQuad::new(6, 0, 9, 12, BorderStyle::Month(1.0)),
            BorderQuad::new(6, 10, 19, 13, BorderStyle::Month(0.5)),
            BorderQuad::new(8, 8, 19, 10, BorderStyle::Selected),
            BorderQuad::new(17, 3, 19, 10, BorderStyle::Selected),
        ];
        let mut expected = BTreeMap::new();
        for stroke in &strokes {
            for pixel in pixels(stroke) {
                let style = expected.entry(pixel).or_insert(BorderStyle::Grid);
                if stroke.style.priority() > style.priority() {
                    *style = stroke.style;
                }
            }
        }
        assert_eq!(rasterize(&merge_strokes(strokes)), expected);
    }

    #[test]
    fn wrapped_selection_and_drop_override_months_without_wrapping_side_neighbors() {
        let start = NaiveDate::from_ymd_opt(2026, 12, 27).unwrap();
        let date = |day| start + chrono::Duration::days(day);
        let mut boundaries = Vec::new();
        let borders = GridBorders::new(
            start,
            3,
            Some(InclusiveDateRange::new(date(4), date(15))),
            Some(date(-1)),
            |month| {
                boundaries.push(month);
                1.0
            },
        );
        assert_eq!(
            boundaries,
            vec![NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()]
        );
        assert_eq!(borders.vertical[0][0], BorderStyle::Grid);
        assert_eq!(borders.vertical[0][7], BorderStyle::Selected);
        assert_eq!(borders.horizontal[1], [BorderStyle::Selected; 7]);
        assert_eq!(borders.horizontal[2], [BorderStyle::Selected; 7]);
        assert_eq!(borders.vertical[1], [BorderStyle::Selected; 8]);
        assert_eq!(
            borders.horizontal[3][0..3],
            [
                BorderStyle::Selected,
                BorderStyle::Selected,
                BorderStyle::Grid
            ]
        );
        assert_eq!(borders.vertical[2][7], BorderStyle::Grid);

        let borders = GridBorders::new(start, 1, None, Some(date(4)), |_| 1.0);
        assert_eq!(borders.vertical[0][5], BorderStyle::Selected);
        assert_eq!(borders.horizontal[1][3], BorderStyle::Month(1.0));
        assert_eq!(borders.horizontal[1][4], BorderStyle::Selected);
        assert_eq!(borders.horizontal[1][5], BorderStyle::Grid);
    }

    #[test]
    fn sidebar_shares_only_the_touching_unselected_edge() {
        let start = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
        let date = |day| start + chrono::Duration::days(day);
        let borders = GridBorders::new(
            start,
            3,
            Some(InclusiveDateRange::new(date(5), date(7))),
            Some(date(14)),
            |_| 1.0,
        );
        let bounds = Bounds::new(point(px(252.4), px(-0.4)), size(px(140.), px(120.)));
        for scale in [1.0, 1.25, 2.0] {
            let raster = |sidebar_right| {
                rasterize(&borders.quads(bounds, px(0.), px(40.), scale, sidebar_right))
            };
            let exposed = raster(None);
            let shared = raster(Some(bounds.left()));
            assert_eq!(raster(Some(bounds.left() - px(20.))), exposed);
            assert_eq!(
                shared
                    .iter()
                    .filter(|(_, style)| **style == BorderStyle::Selected)
                    .collect::<Vec<_>>(),
                exposed
                    .iter()
                    .filter(|(_, style)| **style == BorderStyle::Selected)
                    .collect::<Vec<_>>()
            );

            let left = (bounds.left().as_f32() * scale).ceil() as i32;
            let width = BorderStyle::Grid.width(scale);
            for row in 0..3 {
                let y = ((bounds.top().as_f32() + 40. * row as f32 + 20.) * scale) as i32;
                for x in left..left + width {
                    if row == 0 {
                        assert_eq!(exposed.get(&(x, y)), Some(&BorderStyle::Grid));
                        assert_eq!(shared.get(&(x, y)), None);
                    } else {
                        assert_eq!(shared.get(&(x, y)), Some(&BorderStyle::Selected));
                    }
                }
            }
            assert_eq!(
                shared
                    .iter()
                    .filter(|((x, _), _)| *x >= left + width)
                    .collect::<Vec<_>>(),
                exposed
                    .iter()
                    .filter(|((x, _), _)| *x >= left + width)
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn exposed_edges_shared_edges_and_corners_keep_the_same_full_width() {
        let start = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
        let bounds = Bounds::new(point(px(0.4), px(-0.4)), size(px(140.), px(80.)));
        let row_top = px(-10.);
        let week_height = px(40.);
        for scale in [1.0, 1.25, 2.0] {
            for (first, last) in [(0, 0), (3, 3), (5, 8)] {
                let selection = InclusiveDateRange::new(
                    start + chrono::Duration::days(first),
                    start + chrono::Duration::days(last),
                );
                let borders = GridBorders::new(start, 2, Some(selection), None, |_| 0.0);
                let quads = borders.quads(bounds, row_top, week_height, scale, None);
                let actual: BTreeMap<_, _> = rasterize(&quads)
                    .into_iter()
                    .filter(|(_, style)| *style == BorderStyle::Selected)
                    .collect();
                let width = scale.round() as i32;
                let x = |column| match column {
                    0 => (bounds.left().as_f32() * scale).ceil() as i32,
                    7 => (bounds.right().as_f32() * scale).floor() as i32 - width,
                    _ => ((bounds.left().as_f32() + 20. * column as f32) * scale
                        - width as f32 / 2.)
                        .round() as i32,
                };
                let y = |row| {
                    ((bounds.top() + row_top + week_height * row as f32).as_f32() * scale
                        - width as f32 / 2.)
                        .round() as i32
                };
                let mut expected = BTreeMap::new();
                for day in first..=last {
                    let row = day / 7;
                    let column = day % 7;
                    let (left, right) = (x(column), x(column + 1) + width);
                    let (top, bottom) = (y(row), y(row + 1) + width);
                    for py in top..bottom {
                        for px in left..right {
                            if px < left + width
                                || px >= right - width
                                || py < top + width
                                || py >= bottom - width
                            {
                                expected.insert((px, py), BorderStyle::Selected);
                            }
                        }
                    }
                }
                assert_eq!(
                    actual, expected,
                    "selection {first}..={last}, scale {scale}"
                );
            }
        }
    }
}
