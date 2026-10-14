use gpui_kit::tokens::{TokenDocument, TokenError, contrast};

pub(crate) const FILL_TEXT_MINIMUM: f32 = 3.5;

pub(super) fn parse(json: &str) -> Result<TokenDocument, TokenError> {
    let tokens: TokenDocument = serde_json::from_str(json)?;
    match tokens.validate() {
        Ok(()) => return Ok(tokens),
        Err(TokenError::Contrast(_)) if tokens.meta.id == "subroutine-dark" => {}
        Err(error) => return Err(error),
    }

    let failures: Vec<_> = contrast::report(&tokens)
        .into_iter()
        .map(|mut check| {
            if matches!(
                (check.foreground.as_str(), check.background.as_str()),
                ("color.text.onAccent", "color.semantic.accent")
                    | ("color.text.onPrimaryFill", "color.interactive.primaryFill")
            ) {
                check.minimum = FILL_TEXT_MINIMUM;
            }
            check
        })
        .filter(|check| !check.passes())
        .collect();
    require_empty(failures, TokenError::Contrast)?;

    // validate() stops at contrast; keep its remaining stages in sync when upgrading GPUI Box.
    require_empty(
        contrast::separation_failures(&tokens),
        TokenError::Separation,
    )?;
    require_empty(contrast::line_failures(&tokens), TokenError::Line)?;
    require_empty(
        contrast::placeholder_failures(&tokens),
        TokenError::Placeholder,
    )?;
    require_empty(
        contrast::distinction_failures(&tokens),
        TokenError::Distinction,
    )?;
    require_empty(contrast::series_failures(&tokens), TokenError::Perceptual)?;
    require_empty(contrast::canvas_failures(&tokens), TokenError::Perceptual)?;
    Ok(tokens)
}

fn require_empty<T: std::fmt::Debug>(
    failures: Vec<T>,
    error: fn(String) -> TokenError,
) -> Result<(), TokenError> {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(error(
            failures
                .iter()
                .map(|failure| format!("  {failure:?}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{SUBROUTINE_DARK_JSON, SUBROUTINE_LIGHT_JSON};
    use super::*;
    use gpui_kit::tokens::{
        Appearance, Color, InteractiveColor, SemanticColor, TextTone, contrast_ratio,
    };

    fn parse_tokens(tokens: &TokenDocument) -> Result<TokenDocument, TokenError> {
        parse(&serde_json::to_string(tokens).unwrap())
    }

    fn dark_with_relaxed_fill() -> TokenDocument {
        let mut tokens = parse(SUBROUTINE_DARK_JSON).unwrap();
        tokens.color.interactive.primary_fill = "#C0524E".into();
        assert!(matches!(tokens.validate(), Err(TokenError::Contrast(_))));
        parse_tokens(&tokens).expect("dark fill passes the scoped contrast floor")
    }

    #[test]
    fn subroutine_fill_labels_follow_theme_appearance() {
        for (json, primary, current_time) in [
            (SUBROUTINE_DARK_JSON, "#C65B57", "#DC3540"),
            (SUBROUTINE_LIGHT_JSON, "#B0413E", "#CE2738"),
        ] {
            let tokens = parse(json).expect("Subroutine theme must register");
            let palette = &tokens.color.palette;
            assert_eq!(palette["brandUi"]["primaryFill"], primary);
            assert_eq!(palette["brandUi"]["currentTime"], current_time);
            for (foreground, background) in [
                (
                    tokens.text(TextTone::OnPrimaryFill),
                    tokens.interactive(InteractiveColor::PrimaryFill),
                ),
                (
                    tokens.text(TextTone::OnAccent),
                    tokens.semantic(SemanticColor::Accent),
                ),
                (
                    Color::parse("onCurrentTime", &palette["brandUi"]["onCurrentTime"]).unwrap(),
                    Color::parse("currentTime", &palette["brandUi"]["currentTime"]).unwrap(),
                ),
            ] {
                let minimum = match tokens.meta.appearance {
                    Appearance::Dark => {
                        assert!(foreground.luminance() < background.luminance());
                        FILL_TEXT_MINIMUM
                    }
                    Appearance::Light => {
                        assert!(foreground.luminance() > background.luminance());
                        4.5
                    }
                };
                assert!(contrast_ratio(foreground, background) >= minimum);
            }
        }
    }

    #[test]
    fn contrast_exception_is_limited_to_subroutine_fill_labels() {
        let tokens = dark_with_relaxed_fill();

        let mut too_faint = tokens.clone();
        too_faint.color.text.on_primary_fill = "{brand.charcoal}".into();
        assert!(matches!(
            parse_tokens(&too_faint),
            Err(TokenError::Contrast(_))
        ));

        for id in ["custom-dark", "subroutine-light"] {
            let mut other_theme = tokens.clone();
            other_theme.meta.id = id.into();
            assert!(matches!(
                parse_tokens(&other_theme),
                Err(TokenError::Contrast(_))
            ));
        }

        let mut body_text = tokens;
        body_text.color.text.primary = "#888888".into();
        assert!(matches!(
            parse_tokens(&body_text),
            Err(TokenError::Contrast(_))
        ));
    }

    #[test]
    fn contrast_exception_preserves_structural_and_later_checks() {
        let tokens = dark_with_relaxed_fill();

        let mut invalid = tokens.clone();
        invalid.color.text.on_primary_fill = "{missing.color}".into();
        assert!(matches!(
            parse_tokens(&invalid),
            Err(TokenError::Invalid { .. })
        ));

        let mut invisible_line = tokens;
        invisible_line.color.interactive.divider = "#00000000".into();
        assert!(matches!(
            parse_tokens(&invisible_line),
            Err(TokenError::Line(_))
        ));
    }
}
