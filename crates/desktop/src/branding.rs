use gpui::{
    App, InteractiveElement as _, IntoElement, ObjectFit, ParentElement as _, Styled,
    StyledImage as _, div, img, px,
};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::{ActiveTheme as _, Appearance};

pub(crate) const APP_ID: &str = "com.subroutine.SubroutineLite";

fn logotype_path(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Light => "branding/subroutine_logotype_brick_transparent.svg",
        Appearance::Dark => "branding/subroutine_logotype_cream_transparent.svg",
    }
}

pub(crate) fn logotype(id: &'static str, width: f32, cx: &App) -> impl IntoElement {
    div()
        .id(id)
        .w(px(width))
        .h(px(width * 1024. / 3320.))
        .flex_none()
        .child(
            img(logotype_path(cx.theme().appearance))
                .size_full()
                .object_fit(ObjectFit::Contain),
        )
        .semantic_in(cx, NodeSpec::new(id, Role::Image).text("Subroutine"))
}
