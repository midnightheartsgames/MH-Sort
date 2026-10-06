//! Иконки Phosphor и внешний вид категорий: значок и цвет.

use eframe::egui::{Color32, FontFamily};

egui_phosphor::subset! {
    /// В exe попадают только перечисленные глифы, а не весь шрифт.
    pub mod ph {
        use regular::{
            ARROW_COUNTER_CLOCKWISE, ARROW_RIGHT, ARROW_SQUARE_OUT, ARROWS_CLOCKWISE, CARET_DOWN, CHECK,
            CHECK_SQUARE, CHECKS, EYE_SLASH, FOLDER_OPEN, FOLDER_SIMPLE, GEAR_SIX, INFO,
            MAGNIFYING_GLASS, SPARKLE, SQUARE, STOP, WARNING, X,
        };
        use fill::{
            APP_WINDOW, BOOKS, BRAIN, CHECK_CIRCLE, CODE, CUBE, DISC, FILE_TEXT, FILE_ZIP,
            FILM_STRIP, FOLDER_OPEN, FOLDER_SIMPLE, FOLDERS, GAME_CONTROLLER, IMAGE, LINK, MAGNET,
            MUSIC_NOTES, PAINT_BRUSH, PRESENTATION_CHART, QUESTION, TABLE, TEXT_AA, WARNING_CIRCLE,
        };
    }
}

/// Семейство шрифта для залитых иконок (их надо выбирать явно).
pub fn fill() -> FontFamily {
    ph::fill::family()
}

/// Как категория выглядит в окне.
#[derive(Clone, Copy)]
pub struct Look {
    pub icon: &'static str,
    pub color: Color32,
}

/// Узнаваемые виды файлов: значок, цвет и характерные расширения.
const KINDS: &[(&str, [u8; 3], &[&str])] = &[
    (
        ph::fill::IMAGE,
        [0x3F, 0xD0, 0xC0],
        &[
            "jpg", "jpeg", "png", "webp", "gif", "bmp", "heic", "tif", "svg",
        ],
    ),
    (
        ph::fill::FILM_STRIP,
        [0xA6, 0x8C, 0xFF],
        &["mp4", "mkv", "avi", "mov", "webm", "wmv"],
    ),
    (
        ph::fill::MUSIC_NOTES,
        [0xFF, 0x7C, 0xB5],
        &["mp3", "wav", "flac", "ogg", "m4a", "aac"],
    ),
    (
        ph::fill::FILE_TEXT,
        [0x6F, 0xB4, 0xFF],
        &["txt", "pdf", "doc", "docx", "odt", "rtf", "md"],
    ),
    (
        ph::fill::BOOKS,
        [0xD8, 0xA8, 0x74],
        &["epub", "fb2", "mobi", "djvu", "cbz"],
    ),
    (
        ph::fill::TABLE,
        [0x5B, 0xD9, 0x8C],
        &["xls", "xlsx", "ods", "csv"],
    ),
    (
        ph::fill::PRESENTATION_CHART,
        [0xFF, 0x9F, 0x5A],
        &["ppt", "pptx", "odp"],
    ),
    (
        ph::fill::FILE_ZIP,
        [0xE3, 0xC3, 0x5A],
        &["zip", "rar", "7z", "tar", "gz"],
    ),
    (
        ph::fill::DISC,
        [0x9F, 0xB0, 0xCC],
        &["iso", "img", "vhd", "vhdx"],
    ),
    (
        ph::fill::CUBE,
        [0xFF, 0x82, 0x66],
        &["blend", "fbx", "obj", "gltf", "glb", "stl"],
    ),
    (
        ph::fill::PAINT_BRUSH,
        [0xE5, 0x7B, 0xF0],
        &["psd", "kra", "clip", "xcf", "aep", "prproj"],
    ),
    (
        ph::fill::CODE,
        [0xB4, 0xE0, 0x5A],
        &["py", "js", "ts", "rs", "cpp", "cs", "java", "html"],
    ),
    (
        ph::fill::GAME_CONTROLLER,
        [0xFF, 0x64, 0x70],
        &["pak", "wad", "vpk", "unitypackage", "uasset"],
    ),
    (
        ph::fill::APP_WINDOW,
        [0x8C, 0x9C, 0xFF],
        &["exe", "msi", "apk", "appx", "msix"],
    ),
    (
        ph::fill::BRAIN,
        [0x3C, 0xC8, 0xF0],
        &["safetensors", "gguf", "ckpt", "pt", "onnx"],
    ),
    (ph::fill::MAGNET, [0x34, 0xC7, 0x9A], &["torrent"]),
    (
        ph::fill::TEXT_AA,
        [0xF0, 0xB3, 0xA0],
        &["ttf", "otf", "woff", "woff2"],
    ),
    (ph::fill::LINK, [0xA7, 0xB8, 0xE8], &["lnk", "url"]),
];

/// Цвета для своих категорий, которые ни на что не похожи.
const SPARE_COLORS: [[u8; 3]; 6] = [
    [0x7F, 0xD1, 0xE8],
    [0xC7, 0xA1, 0xFF],
    [0xFF, 0xB3, 0x8A],
    [0x9B, 0xE3, 0x9B],
    [0xF2, 0xA2, 0xC8],
    [0xA0, 0xB4, 0xFF],
];

/// Категория для неопознанных файлов.
pub const UNKNOWN: Look = Look {
    icon: ph::fill::QUESTION,
    color: Color32::from_rgb(0x84, 0x90, 0xAB),
};

/// Значок и цвет по расширениям категории: переименованная категория выглядит так же.
pub fn look_for(name: &str, extensions: &[String]) -> Look {
    let mut best: Option<(usize, &str, [u8; 3])> = None;
    for &(icon, rgb, known) in KINDS {
        let hits = extensions
            .iter()
            .filter(|ext| {
                known.contains(&ext.trim().trim_start_matches('.').to_lowercase().as_str())
            })
            .count();
        if hits > best.map_or(0, |(n, ..)| n) {
            best = Some((hits, icon, rgb));
        }
    }
    let (icon, [r, g, b]) = match best {
        Some((_, icon, rgb)) => (icon, rgb),
        None => {
            let hash = name
                .chars()
                .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
            (
                ph::fill::FOLDER_SIMPLE,
                SPARE_COLORS[hash as usize % SPARE_COLORS.len()],
            )
        }
    };
    Look {
        icon,
        color: Color32::from_rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exts(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn look_follows_extensions_not_name() {
        assert_eq!(
            look_for("Картинки", &exts(&["JPG", ".png"])).icon,
            ph::fill::IMAGE
        );
        assert_eq!(
            look_for("Модели", &exts(&["blend", "fbx", "png"])).icon,
            ph::fill::CUBE
        );
        let custom = look_for("Разное", &exts(&["xyz"]));
        assert_eq!(custom.icon, ph::fill::FOLDER_SIMPLE);
        assert_eq!(custom.color, look_for("Разное", &[]).color);
    }
}
