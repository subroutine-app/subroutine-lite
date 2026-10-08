use anyhow::Result;
use gpui::{AssetSource, SharedString};
use rust_embed::RustEmbed;
use std::borrow::Cow;

#[derive(RustEmbed)]
#[folder = "../../assets"]
#[include = "icons/**/*.svg"]
#[include = "images/**/*.png"]
pub struct Embedded;

#[derive(RustEmbed)]
#[folder = "resources"]
#[include = "branding/*.svg"]
#[include = "branding/*.png"]
struct DesktopBranding;

#[derive(Debug)]
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(file) = DesktopBranding::get(path).or_else(|| Embedded::get(path)) {
            return Ok(Some(file.data));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths: Vec<SharedString> = Embedded::iter()
            .filter(|p| p.starts_with(path))
            .map(|p| p.into())
            .collect();
        paths.extend(
            DesktopBranding::iter()
                .filter(|p| p.starts_with(path))
                .map(Into::into),
        );
        paths.extend(gpui_kit::assets::Assets.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
