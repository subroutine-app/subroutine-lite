
use gpui::{App, Bounds, Entity, EntityInputHandler as _, Pixels, Size, Window};
use gpui_kit::controls::input::TextInput;
use std::ops::Range;

fn utf16_offset(text: &str, byte_offset: usize) -> usize {
    let byte_offset = byte_offset.min(text.len());
    let mut boundary = byte_offset;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text[..boundary].encode_utf16().count()
}

pub fn range_bounds(
    input: &Entity<TextInput>,
    range: &Range<usize>,
    field_size: Size<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> Option<Bounds<Pixels>> {
    let text = input.read(cx).value().to_string();
    let start = utf16_offset(&text, range.start);
    let end = utf16_offset(&text, range.end);
    if start >= end {
        return None;
    }

    let field = Bounds {
        origin: gpui::point(Pixels::ZERO, Pixels::ZERO),
        size: field_size,
    };

    input.update(cx, |input, cx| {
        input.bounds_for_range(start..end, field, window, cx)
    })
}
