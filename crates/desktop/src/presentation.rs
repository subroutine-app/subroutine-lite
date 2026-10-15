use gpui::Hsla;
use gpui_kit_theme::{Appearance, SemanticBorder, SemanticWash, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UxColor {
    Neutral,
    Selected,
    Context,
    Action,
    Attention,
    Stable,
    Critical,
    CurrentTime,
}

impl UxColor {
    pub(crate) fn color(self, theme: &Theme) -> Hsla {
        match self {
            Self::Neutral => theme.colors.text_muted,
            Self::Selected => theme.colors.accent,
            Self::Context => theme.colors.info,
            Self::Action => theme
                .palette_color("brandUi.action")
                .unwrap_or(theme.colors.warning),
            Self::Attention => theme
                .palette_color("brandUi.attention")
                .unwrap_or(theme.colors.warning),
            Self::Stable => theme.colors.success,
            Self::Critical => theme.colors.danger,
            Self::CurrentTime => theme
                .palette_color("brandUi.currentTime")
                .unwrap_or_else(|| {
                    let shades = match theme.appearance {
                        Appearance::Light => ["900", "800", "700", "600", "500", "400", "300"],
                        Appearance::Dark => ["700", "600", "500", "400", "300", "800", "900"],
                    };
                    shades
                        .into_iter()
                        .filter_map(|step| theme.palette_color(&format!("red.{step}")))
                        .map(|red| red.blend(theme.colors.canvas.opacity(0.06)))
                        .find(|red| {
                            surfaces(theme)
                                .into_iter()
                                .all(|surface| theme.contrast(*red, surface) >= 3.0)
                        })
                        .unwrap_or(theme.colors.danger)
                }),
        }
    }

    pub(crate) fn text(self, theme: &Theme) -> Hsla {
        let candidate = match self {
            Self::Selected => theme.colors.accent_strong,
            Self::Context => theme
                .palette_color("brandUi.contextStrong")
                .unwrap_or_else(|| self.color(theme)),
            Self::Action => theme
                .palette_color("brandUi.actionText")
                .unwrap_or_else(|| self.color(theme)),
            Self::CurrentTime => theme
                .palette_color("brandUi.currentTimeText")
                .unwrap_or_else(|| self.color(theme)),
            Self::Attention => theme.colors.text,
            _ => self.color(theme),
        };
        let washes = match self {
            Self::Selected => [
                theme.colors.selected,
                theme.colors.selected.blend(theme.colors.hover),
                theme.colors.selected.blend(theme.colors.active),
            ],
            _ => [self.wash(theme), theme.colors.hover, theme.colors.active],
        };
        let readable = surfaces(theme).into_iter().all(|surface| {
            theme.contrast(candidate, surface) >= 4.5
                && washes
                    .into_iter()
                    .all(|wash| theme.contrast(candidate, surface.blend(wash)) >= 4.5)
        });
        if readable {
            candidate
        } else {
            theme.colors.text
        }
    }

    pub(crate) fn wash(self, theme: &Theme) -> Hsla {
        theme.color_wash(self.color(theme), SemanticWash::Faint)
    }

    pub(crate) fn on_fill(self, theme: &Theme) -> Hsla {
        let fill = self.color(theme);
        if self == Self::CurrentTime
            && let Some(foreground) = theme.palette_color("brandUi.onCurrentTime")
            && theme.contrast(foreground, fill) >= 4.5
        {
            return foreground;
        }
        let foreground = theme.readable_on(fill);
        if theme.contrast(foreground, fill) >= 4.5 {
            return foreground;
        }
        if theme.contrast(gpui::black(), fill) > theme.contrast(gpui::white(), fill) {
            gpui::black()
        } else {
            gpui::white()
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RecognitionPaint {
    pub(crate) fill: Hsla,
    pub(crate) border: Hsla,
}

impl RecognitionPaint {
    pub(crate) fn new(theme: &Theme) -> Self {
        let context = UxColor::Context.color(theme);
        Self {
            fill: theme
                .palette_color("recognition.fill")
                .unwrap_or_else(|| theme.color_wash(context, SemanticWash::Strong)),
            border: theme
                .palette_color("recognition.border")
                .unwrap_or_else(|| theme.color_border(context, SemanticBorder::Target)),
        }
    }
}

fn surfaces(theme: &Theme) -> [Hsla; 9] {
    [
        theme.colors.backdrop,
        theme.colors.canvas,
        theme.colors.sunken,
        theme.colors.panel,
        theme.colors.raised,
        theme.colors.overlay,
        theme.colors.control,
        theme.colors.control_hover,
        theme.colors.control_pressed,
    ]
}
