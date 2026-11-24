//! Bundled Latin and Arabic fonts.
//!
//! The variable TTFs are embedded in the binary and registered before Slint initializes
//! the platform, so both scripts render without depending on installed system fonts.

use std::sync::Arc;

use slint::fontique_012::fontique;

const INTER: &[u8] = include_bytes!("../../../assets/fonts/Inter-Variable.ttf");
const NOTO_ARABIC: &[u8] = include_bytes!("../../../assets/fonts/NotoSansArabic-Variable.ttf");

/// Register the bundled fonts as the sans-serif family and add an Arabic fallback.
pub fn register() -> Result<(), slint::PlatformError> {
    let mut collection = fontique::Collection::new(fontique::CollectionOptions {
        system_fonts: false,
        ..Default::default()
    });

    let inter = collection.register_fonts(fontique::Blob::new(Arc::new(INTER.to_vec())), None);
    let arabic =
        collection.register_fonts(fontique::Blob::new(Arc::new(NOTO_ARABIC.to_vec())), None);

    let sans: Vec<_> = inter
        .iter()
        .chain(arabic.iter())
        .map(|(family, _)| *family)
        .collect();
    collection.set_generic_families(fontique::GenericFamily::SansSerif, sans.iter().copied());

    collection.append_fallbacks(
        fontique::FallbackKey::new(fontique::Script::from_bytes(*b"Arab"), None),
        arabic.iter().map(|(family, _)| *family),
    );

    slint::fontique_012::set_shared_collection(collection)
}
