//! Виджеты в стиле «Полночь»: галочка, переключатель, сегменты, главная кнопка,
//! вкладка, плашка, строка категории и пунктирная рамка.

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Button, Color32, FontId, Galley, Painter, Pos2, Rect, Response, RichText, Sense,
    Shape, Stroke, StrokeKind, Ui, Vec2, Widget, WidgetInfo, WidgetType, lerp, pos2, vec2,
};
use egui::text::{LayoutJob, TextWrapping};

use crate::icons::{self, Look};
use crate::theme::{
    self, FAINT, HOVER, LINE, MOON, MOON_HOVER, MOON_PRESSED, MUTED, ON_MOON, RAISED, SELECTED,
    TEXT,
};

/// Однострочный текст, обрезанный многоточием по ширине.
pub fn single_line(ui: &Ui, text: impl Into<String>, font: FontId, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.into(), font, Color32::PLACEHOLDER);
    job.wrap = TextWrapping::truncate_at_width(width.max(10.0));
    ui.painter().layout_job(job)
}

pub fn focus_ring(painter: &Painter, rect: Rect, radius: f32) {
    painter.rect_stroke(
        rect.expand(2.5),
        radius + 2.5,
        Stroke::new(1.5, MOON),
        StrokeKind::Outside,
    );
}

/// Галочка без видимой подписи; имя нужно экранному диктору.
pub struct Check<'a> {
    checked: &'a mut bool,
    name: String,
}

impl<'a> Check<'a> {
    pub fn new(checked: &'a mut bool, name: impl Into<String>) -> Self {
        Self {
            checked,
            name: name.into(),
        }
    }
}

impl Widget for Check<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, mut response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
        if response.clicked() {
            *self.checked = !*self.checked;
            response.mark_changed();
        }
        let checked = *self.checked;
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), checked, &self.name)
        });
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let hot = response.hovered() || response.has_focus();
            if checked {
                // Спокойная заливка: золото остаётся за главной кнопкой и выбором
                let fill = if hot {
                    Color32::from_rgb(0x55, 0x67, 0x92)
                } else {
                    Color32::from_rgb(0x46, 0x57, 0x80)
                };
                painter.rect_filled(rect, 5.0, fill);
                let c = rect.center();
                let mark = vec![
                    pos2(c.x - 4.2, c.y + 0.3),
                    pos2(c.x - 1.3, c.y + 3.2),
                    pos2(c.x + 4.3, c.y - 3.1),
                ];
                painter.add(Shape::line(mark, Stroke::new(2.0, TEXT)));
            } else {
                painter.rect_filled(rect, 5.0, if hot { HOVER } else { RAISED });
                let border = if hot {
                    MUTED
                } else {
                    Color32::from_rgb(0x3A, 0x48, 0x6B)
                };
                painter.rect_stroke(rect, 5.0, Stroke::new(1.0, border), StrokeKind::Inside);
            }
            if response.has_focus() {
                focus_ring(painter, rect, 5.0);
            }
        }
        response
    }
}

const TRACK: Vec2 = vec2(34.0, 18.0);

fn paint_switch(ui: &Ui, track: Rect, response: &Response, on: bool) {
    let t = ui.ctx().animate_bool_responsive(response.id, on);
    let painter = ui.painter();
    let off_fill = if response.hovered() { HOVER } else { RAISED };
    painter.rect_filled(track, 9.0, off_fill.lerp_to_gamma(MOON, t));
    if t < 1.0 {
        painter.rect_stroke(
            track,
            9.0,
            Stroke::new(1.0, LINE.gamma_multiply(1.0 - t)),
            StrokeKind::Inside,
        );
    }
    let x = lerp(track.left() + 9.0..=track.right() - 9.0, t);
    painter.circle_filled(
        pos2(x, track.center().y),
        6.0,
        MUTED.lerp_to_gamma(ON_MOON, t),
    );
    if response.has_focus() {
        focus_ring(painter, track, 9.0);
    }
}

/// Переключатель с подписью и пояснением во всю ширину — для настроек, которые применяются сразу.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str, hint: Option<&str>) -> Response {
    let gap = 10.0;
    let width = ui.available_width();
    let text_width = (width - TRACK.x - gap).max(60.0);
    let painter = ui.painter();
    let label_galley = painter.layout(
        label.to_owned(),
        FontId::proportional(14.0),
        Color32::PLACEHOLDER,
        text_width,
    );
    let hint_galley = hint.map(|h| {
        painter.layout(
            h.to_owned(),
            FontId::proportional(12.5),
            Color32::PLACEHOLDER,
            text_width,
        )
    });
    let first_line = ui.text_style_height(&egui::TextStyle::Body);
    let text_height =
        label_galley.size().y + hint_galley.as_ref().map_or(0.0, |g| g.size().y + 2.0);
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(width, text_height.max(TRACK.y) + 4.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let checked = *on;
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), checked, label)
    });
    let track = Rect::from_min_size(
        pos2(
            rect.left(),
            rect.top() + 2.0 + (first_line - TRACK.y).max(0.0) / 2.0,
        ),
        TRACK,
    );
    paint_switch(ui, track, &response, checked);
    let text_left = track.right() + gap;
    let painter = ui.painter();
    let label_height = label_galley.size().y;
    painter.galley(pos2(text_left, rect.top() + 2.0), label_galley, TEXT);
    if let Some(galley) = hint_galley {
        painter.galley(
            pos2(text_left, rect.top() + 4.0 + label_height),
            galley,
            MUTED,
        );
    }
    response
}

/// Компактный переключатель для строки инструментов.
pub fn switch_inline(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let gap = 8.0;
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(14.0),
        Color32::PLACEHOLDER,
    );
    let size = vec2(
        TRACK.x + gap + galley.size().x,
        TRACK.y.max(galley.size().y),
    );
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let checked = *on;
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), checked, label)
    });
    let track = Rect::from_min_size(pos2(rect.left(), rect.center().y - TRACK.y / 2.0), TRACK);
    paint_switch(ui, track, &response, checked);
    let text_pos = pos2(track.right() + gap, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(text_pos, galley, TEXT);
    response
}

/// Переключатель из нескольких вариантов, например «Переместить | Копировать».
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, &str)],
) -> Response {
    let font = FontId::proportional(14.0);
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, text)| {
            ui.painter()
                .layout_no_wrap(text.to_string(), font.clone(), Color32::PLACEHOLDER)
        })
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 28.0).collect();
    let height = 34.0;
    let (rect, mut response) = ui.allocate_exact_size(
        vec2(widths.iter().sum::<f32>() + 6.0, height),
        Sense::hover(),
    );
    ui.painter().rect_filled(rect, 9.0, RAISED);
    let mut x = rect.left() + 3.0;
    for (index, ((option, text), galley)) in options.iter().zip(galleys).enumerate() {
        let segment =
            Rect::from_min_size(pos2(x, rect.top() + 3.0), vec2(widths[index], height - 6.0));
        x += widths[index];
        let part = ui.interact(segment, response.id.with(index), Sense::click());
        let selected = *value == *option;
        part.widget_info(|| {
            WidgetInfo::selected(WidgetType::RadioButton, ui.is_enabled(), selected, *text)
        });
        if part.clicked() && !selected {
            *value = *option;
            response.mark_changed();
        }
        let selected = *value == *option;
        let painter = ui.painter();
        if selected {
            painter.rect_filled(segment, 7.0, SELECTED);
        } else if part.hovered() {
            painter.rect_filled(segment, 7.0, HOVER);
        }
        let color = if selected { MOON } else { MUTED };
        painter.galley(segment.center() - galley.size() / 2.0, galley, color);
        if part.has_focus() {
            focus_ring(painter, segment, 7.0);
        }
    }
    response
}

/// Главная кнопка экрана: золотая, одна на экран.
pub fn primary(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        theme::bold_font(15.0),
        Color32::PLACEHOLDER,
    );
    let size = vec2((galley.size().x + 40.0).max(200.0), 38.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, text));
    let fill = if !enabled {
        RAISED
    } else if response.is_pointer_button_down_on() {
        MOON_PRESSED
    } else if response.hovered() {
        MOON_HOVER
    } else {
        MOON
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 9.0, fill);
    painter.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        if enabled { ON_MOON } else { FAINT },
    );
    if response.has_focus() {
        focus_ring(painter, rect, 9.0);
    }
    response
}

/// Кнопка-иконка без рамки (рамка появляется при наведении).
pub fn icon_button(icon: &str) -> Button<'_> {
    Button::new(RichText::new(icon).size(17.0))
        .frame_when_inactive(false)
        .min_size(vec2(32.0, 32.0))
}

/// Вкладка с золотой чертой снизу; `badge` — число ошибок.
pub fn tab(ui: &mut Ui, selected: bool, text: &str, badge: Option<usize>) -> Response {
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        theme::bold_font(14.0),
        Color32::PLACEHOLDER,
    );
    let badge_galley = badge.map(|n| {
        painter.layout_no_wrap(n.to_string(), theme::bold_font(11.5), Color32::PLACEHOLDER)
    });
    let badge_width = badge_galley.as_ref().map_or(0.0, |g| g.size().x + 18.0);
    let (rect, response) = ui.allocate_exact_size(
        vec2(galley.size().x + 20.0 + badge_width, 34.0),
        Sense::click(),
    );
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, text));
    let color = if selected || response.hovered() {
        TEXT
    } else {
        MUTED
    };
    let painter = ui.painter();
    let text_pos = pos2(
        rect.left() + 10.0,
        rect.center().y - galley.size().y / 2.0 - 1.0,
    );
    let text_right = text_pos.x + galley.size().x;
    painter.galley(text_pos, galley, color);
    if let Some(number) = badge_galley {
        let pill = Rect::from_min_size(
            pos2(text_right + 6.0, rect.center().y - 10.0),
            vec2(number.size().x + 12.0, 18.0),
        );
        painter.rect_filled(pill, 9.0, theme::ERROR.gamma_multiply(0.22));
        painter.galley(pill.center() - number.size() / 2.0, number, theme::ERROR);
    }
    if selected {
        let bar = Rect::from_min_max(
            pos2(rect.left() + 8.0, rect.bottom() - 3.0),
            pos2(rect.right() - 8.0, rect.bottom()),
        );
        painter.rect_filled(bar, 1.5, MOON);
    }
    if response.has_focus() {
        focus_ring(painter, rect, 6.0);
    }
    response
}

/// Небольшая плашка со значком: «На месте: 20».
pub fn badge(ui: &mut Ui, icon: &str, text: &str, color: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(
        format!("{icon}  {text}"),
        FontId::proportional(12.5),
        Color32::PLACEHOLDER,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(18.0, 8.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, color.gamma_multiply(0.14));
    painter.galley(rect.min + vec2(9.0, 4.0), galley, color);
    response
}

/// Значок категории: цветной глиф на тонированной подложке.
pub fn paint_icon_chip(painter: &Painter, rect: Rect, look: &Look, dim: bool) {
    let color = if dim { FAINT } else { look.color };
    painter.rect_filled(rect, rect.height() * 0.3, color.gamma_multiply(0.16));
    let font = FontId::new(rect.height() * 0.64, icons::fill());
    painter.text(rect.center(), Align2::CENTER_CENTER, look.icon, font, color);
}

/// Строка списка категорий. Возвращает ответ строки и признак, что галочку переключили.
pub fn category_row(
    ui: &mut Ui,
    look: &Look,
    name: &str,
    count: &str,
    active: bool,
    include: Option<&mut bool>,
) -> (Response, bool) {
    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    let dim = include.as_ref().is_some_and(|on| !**on);
    if active {
        ui.painter().rect_filled(rect, 8.0, SELECTED);
        let marker = Rect::from_min_size(
            pos2(rect.left(), rect.top() + 8.0),
            vec2(3.0, rect.height() - 16.0),
        );
        ui.painter().rect_filled(marker, 1.5, MOON);
    } else if row.hovered() {
        ui.painter()
            .rect_filled(rect, 8.0, HOVER.gamma_multiply(0.6));
    }
    let mut changed = false;
    if let Some(on) = include {
        let check =
            Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(18.0, 18.0));
        changed = ui.put(check, Check::new(on, name)).changed();
    }
    let chip = Rect::from_center_size(pos2(rect.left() + 50.0, rect.center().y), vec2(24.0, 24.0));
    paint_icon_chip(ui.painter(), chip, look, dim);

    let count_galley = ui.painter().layout_no_wrap(
        count.to_owned(),
        FontId::proportional(13.0),
        Color32::PLACEHOLDER,
    );
    let name_left = chip.right() + 10.0;
    let name_width = rect.right() - 10.0 - count_galley.size().x - 10.0 - name_left;
    let font = if active {
        theme::bold_font(14.0)
    } else {
        FontId::proportional(14.0)
    };
    let name_galley = single_line(ui, name, font, name_width);
    let painter = ui.painter();
    let name_color = if dim { FAINT } else { TEXT };
    painter.galley(
        pos2(name_left, rect.center().y - name_galley.size().y / 2.0),
        name_galley,
        name_color,
    );
    let count_pos = pos2(
        rect.right() - 10.0 - count_galley.size().x,
        rect.center().y - count_galley.size().y / 2.0,
    );
    painter.galley(count_pos, count_galley, if dim { FAINT } else { MUTED });
    if row.has_focus() {
        focus_ring(painter, rect, 8.0);
    }
    row.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            true,
            active,
            format!("{name}: {count}"),
        )
    });
    (row, changed)
}

/// Пунктирная скруглённая рамка — зона, куда можно бросить папку.
pub fn dashed_frame(painter: &Painter, rect: Rect, radius: f32, stroke: Stroke) {
    let mut points: Vec<Pos2> = Vec::new();
    let corners = [
        (pos2(rect.right() - radius, rect.top() + radius), -90.0f32),
        (pos2(rect.right() - radius, rect.bottom() - radius), 0.0),
        (pos2(rect.left() + radius, rect.bottom() - radius), 90.0),
        (pos2(rect.left() + radius, rect.top() + radius), 180.0),
    ];
    for (center, start) in corners {
        for step in 0..=8 {
            let angle = (start + step as f32 * 90.0 / 8.0).to_radians();
            points.push(center + vec2(angle.cos(), angle.sin()) * radius);
        }
    }
    points.push(points[0]);
    painter.extend(Shape::dashed_line(&points, stroke, 8.0, 6.0));
}

pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(16.0);
    ui.label(
        RichText::new(title)
            .font(theme::bold_font(14.0))
            .color(TEXT),
    );
    ui.add_space(4.0);
}

pub fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.5).color(MUTED));
}
