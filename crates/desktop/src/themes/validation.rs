use gpui_kit::tokens::{TokenDocument, TokenError};

pub(super) fn parse(json: &str) -> Result<TokenDocument, TokenError> {
    TokenDocument::parse(json)
}

#[cfg(test)]
mod tests {
    use super::super::{SUBROUTINE_DARK_JSON, SUBROUTINE_LIGHT_JSON};
    use super::*;
    use crate::presentation::UxColor;
    use gpui_kit_theme::{Density, Theme, Variant};

    #[test]
    fn subroutine_roles_stay_readable_across_surfaces_and_interaction_states() {
        for json in [SUBROUTINE_LIGHT_JSON, SUBROUTINE_DARK_JSON] {
            let tokens = parse(json).expect("Subroutine theme must pass all token checks");
            let theme = Theme::from_tokens(&tokens, Density::Comfortable);
            let colors = &theme.colors;
            let surfaces = [
                colors.backdrop,
                colors.canvas,
                colors.sunken,
                colors.panel,
                colors.raised,
                colors.overlay,
                colors.control,
                colors.control_hover,
                colors.control_pressed,
            ];
            for role in [
                UxColor::Selected,
                UxColor::Context,
                UxColor::Action,
                UxColor::Attention,
                UxColor::Stable,
                UxColor::Critical,
                UxColor::CurrentTime,
            ] {
                if role != UxColor::Attention {
                    assert_ne!(
                        role.text(&theme),
                        colors.text,
                        "{}: {role:?} lost its hue",
                        tokens.meta.id
                    );
                }
                for surface in surfaces {
                    assert!(
                        theme.contrast(role.color(&theme), surface) >= 3.0,
                        "{}: {role:?} marker",
                        tokens.meta.id,
                    );
                    for wash in [role.wash(&theme), colors.hover, colors.active] {
                        assert!(
                            theme.contrast(role.text(&theme), surface.blend(wash)) >= 4.5,
                            "{}: {role:?} text on wash or interaction state",
                            tokens.meta.id,
                        );
                    }
                }
                assert!(
                    theme.contrast(role.on_fill(&theme), role.color(&theme)) >= 4.5,
                    "{}: {role:?} fill label",
                    tokens.meta.id,
                );
            }

            let selected_text = UxColor::Selected.text(&theme);
            assert_eq!(selected_text, colors.accent_strong);
            for wash in [
                colors.selected,
                colors.selected.blend(colors.hover),
                colors.selected.blend(colors.active),
            ] {
                for surface in surfaces {
                    assert!(
                        theme.contrast(selected_text, surface.blend(wash)) >= 4.5,
                        "{}: selected control text",
                        tokens.meta.id,
                    );
                }
            }

            let primary = theme.variant_colors(Variant::Filled, &colors.primary_fill.into());
            for fill in [
                primary.background,
                primary.background_hover,
                primary.background_active,
            ] {
                assert!(
                    theme.contrast(colors.text_on_primary_fill, fill) >= 4.5,
                    "{}: primary button label",
                    tokens.meta.id,
                );
            }
            assert_ne!(colors.primary_fill, colors.accent);
        }
    }

    #[test]
    fn completion_controls_remain_readable_in_gpui_box_themes() {
        for tokens in gpui_kit::tokens::all() {
            let theme = Theme::from_tokens(tokens, Density::Comfortable);
            let text = UxColor::Stable.text(&theme);
            for wash in [
                gpui::transparent_black(),
                theme.colors.hover,
                theme.colors.active,
            ] {
                assert!(
                    theme.contrast(text, theme.colors.overlay.blend(wash)) >= 4.5,
                    "{}: completion control",
                    tokens.meta.id,
                );
            }
        }
    }

    #[test]
    fn subroutine_themes_do_not_bypass_fill_contrast_validation() {
        for json in [SUBROUTINE_LIGHT_JSON, SUBROUTINE_DARK_JSON] {
            let mut tokens = parse(json).unwrap();
            tokens.color.text.on_primary_fill = tokens.color.interactive.primary_fill.clone();
            assert!(matches!(
                parse(&serde_json::to_string(&tokens).unwrap()),
                Err(TokenError::Contrast(_)),
            ));
        }
    }
}
