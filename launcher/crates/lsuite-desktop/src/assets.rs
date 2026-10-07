//! Bundled assets: lucide icons (ISC, `assets/icons/LICENSE.lucide.txt`), the launcher's mark,
//! the apps' icons (from `lsuite-core`, served as `apps/<app>.png`) and the interface faces
//! (Chakra Petch and IBM Plex Mono, OFL, `assets/fonts/`).

use std::borrow::Cow;

use gpui::{App, AssetSource, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/*.svg"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some(id) = path.strip_prefix("apps/").and_then(|p| p.strip_suffix(".png")) {
            return Ok(lsuite_core::app_icon(id).map(Cow::Borrowed));
        }
        if path == "brand/lsuite.png" {
            return Ok(Some(Cow::Borrowed(include_bytes!("../resources/lsuite.png"))));
        }
        Ok(Self::get(path).map(|f| f.data))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Self::iter().filter(|p| p.starts_with(path)).map(SharedString::from).collect())
    }
}

const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/ChakraPetch-Regular.ttf"),
    include_bytes!("../assets/fonts/ChakraPetch-Medium.ttf"),
    include_bytes!("../assets/fonts/ChakraPetch-SemiBold.ttf"),
    include_bytes!("../assets/fonts/ChakraPetch-Bold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Bold.ttf"),
];

pub fn load_fonts(cx: &mut App) {
    if let Err(e) = cx.text_system().add_fonts(FONTS.iter().map(|f| Cow::Borrowed(*f)).collect()) {
        tracing::warn!("couldn't load the bundled fonts: {e}");
    }
}

/// An icon by lucide name, e.g. `icon_path("download")`.
pub fn icon_path(name: &str) -> SharedString {
    format!("icons/{name}.svg").into()
}

/// An app's icon (`apps/<app>.png`).
pub fn app_icon(id: &str) -> SharedString {
    format!("apps/{id}.png").into()
}
