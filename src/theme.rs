//! Оформление «Полночь»: ночные синие поверхности, лунно-золотой акцент,
//! системный шрифт Windows в двух начертаниях и иконки Phosphor.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, vec2,
};

use crate::icons::ph;

/// Шапка, подвал и панель настроек.
pub const CHROME: Color32 = Color32::from_rgb(0x0F, 0x16, 0x29);
/// Рабочая область.
pub const SURFACE: Color32 = Color32::from_rgb(0x14, 0x1C, 0x31);
/// Поля, карточки, неактивные кнопки.
pub const RAISED: Color32 = Color32::from_rgb(0x1C, 0x26, 0x40);
pub const HOVER: Color32 = Color32::from_rgb(0x26, 0x31, 0x4F);
/// Выбранный элемент списка или сегмента.
pub const SELECTED: Color32 = Color32::from_rgb(0x2C, 0x39, 0x5C);
pub const LINE: Color32 = Color32::from_rgb(0x2A, 0x36, 0x55);
pub const TEXT: Color32 = Color32::from_rgb(0xE7, 0xEC, 0xF7);
pub const MUTED: Color32 = Color32::from_rgb(0x93, 0xA0, 0xBF);
pub const FAINT: Color32 = Color32::from_rgb(0x5A, 0x67, 0x88);
/// Акцент: главное действие, выбор, фокус.
pub const MOON: Color32 = Color32::from_rgb(0xF3, 0xC9, 0x69);
pub const MOON_HOVER: Color32 = Color32::from_rgb(0xF8, 0xD8, 0x8A);
pub const MOON_PRESSED: Color32 = Color32::from_rgb(0xDD, 0xB2, 0x52);
/// Текст на золотом фоне.
pub const ON_MOON: Color32 = Color32::from_rgb(0x22, 0x1A, 0x08);
pub const OK: Color32 = Color32::from_rgb(0x6F, 0xD9, 0xA3);
pub const WARN: Color32 = Color32::from_rgb(0xFF, 0xB0, 0x60);
pub const ERROR: Color32 = Color32::from_rgb(0xFF, 0x7A, 0x85);

/// Полужирное начертание: в egui это отдельное семейство.
pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn bold_font(size: f32) -> FontId {
    FontId::new(size, bold())
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    // Тема задумана тёмной: светлый вариант не поддерживается
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, style);
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    // Segoe UI — системный шрифт Windows: чёткая кириллица на мелких размерах
    #[cfg(windows)]
    if let Some(windows) = std::env::var_os("SystemRoot") {
        let dir = std::path::Path::new(&windows).join("Fonts");
        for (name, file) in [
            ("Segoe UI", "segoeui.ttf"),
            ("Segoe UI Semibold", "seguisb.ttf"),
        ] {
            if let Ok(bytes) = std::fs::read(dir.join(file)) {
                fonts
                    .font_data
                    .insert(name.into(), Arc::new(FontData::from_owned(bytes)));
            }
        }
    }
    let has = |name: &str| fonts.font_data.contains_key(name);
    let (regular, semibold) = (has("Segoe UI"), has("Segoe UI Semibold"));
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    if regular {
        proportional.insert(0, "Segoe UI".into());
    }
    ph::regular::add_to_fonts(&mut fonts);
    ph::fill::add_as_family(&mut fonts);

    let mut bold_list = fonts.families[&FontFamily::Proportional].clone();
    if semibold {
        if regular {
            bold_list[0] = "Segoe UI Semibold".into();
        } else {
            bold_list.insert(0, "Segoe UI Semibold".into());
        }
    }
    fonts.families.insert(bold(), bold_list);
    fonts
}

fn style(style: &mut egui::Style) {
    use FontFamily::Proportional;
    style.text_styles = [
        (TextStyle::Small, FontId::new(12.0, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Heading, FontId::new(18.0, bold())),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();

    let spacing = &mut style.spacing;
    spacing.item_spacing = vec2(8.0, 6.0);
    spacing.button_padding = vec2(12.0, 5.0);
    spacing.interact_size.y = 28.0;
    spacing.menu_margin = Margin::same(6);

    let v = &mut style.visuals;
    v.panel_fill = CHROME;
    v.window_fill = RAISED;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(110),
    };
    // Поля ввода темнее рабочей области, полосы таблицы — чуть светлее
    v.extreme_bg_color = Color32::from_rgb(0x0C, 0x12, 0x22);
    v.faint_bg_color = Color32::from_rgb(0x18, 0x21, 0x39);
    v.code_bg_color = RAISED;
    v.hyperlink_color = MOON;
    v.warn_fg_color = WARN;
    v.error_fg_color = ERROR;
    v.weak_text_color = Some(MUTED);
    v.selection.bg_fill = SELECTED;
    v.selection.stroke = Stroke::new(1.0, MOON);
    v.text_cursor.stroke = Stroke::new(2.0, MOON);
    v.striped = true;
    v.indent_has_left_vline = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = CHROME;
    w.noninteractive.weak_bg_fill = CHROME;
    w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.noninteractive.corner_radius = CornerRadius::same(6);
    for (state, fill, stroke, text) in [
        (
            &mut w.inactive,
            RAISED,
            Stroke::NONE,
            Stroke::new(1.0, TEXT),
        ),
        (
            &mut w.hovered,
            HOVER,
            Stroke::new(1.0, Color32::from_rgb(0x3B, 0x4A, 0x70)),
            Stroke::new(1.5, TEXT),
        ),
        (
            &mut w.active,
            SELECTED,
            Stroke::new(1.0, MOON),
            Stroke::new(2.0, Color32::WHITE),
        ),
        (
            &mut w.open,
            HOVER,
            Stroke::new(1.0, LINE),
            Stroke::new(1.0, TEXT),
        ),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = stroke;
        state.fg_stroke = text;
        state.corner_radius = CornerRadius::same(6);
        state.expansion = 0.0;
    }
}
