//! Окно программы.
//!
//! Экран сверху вниз повторяет задачу: шапка (какую папку разбираем) →
//! сводка (что в ней и куда поедет) → категории и файлы (проверить и поправить) →
//! подвал (как и куда, главная кнопка). Редкие настройки — в выдвижной панели справа.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Button, CornerRadius, FontId, Frame, Id, Layout, Margin, Rect, RichText,
    Sense, Stroke, TextEdit, Ui, UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};
use egui::text::{LayoutJob, TextFormat};
use egui_extras::{Column, TableBuilder};

use crate::classify::Classifier;
use crate::config::{Config, Paths, Settings};
use crate::history::{self, Journal};
use crate::icons::{self, Look, ph};
use crate::scanner::{self, Plan, PlannedMove, ScanOptions};
use crate::sorter::{self, Action, Mode, Progress, Report, SortJob};
use crate::theme::{
    self, CHROME, ERROR, FAINT, LINE, MOON, MUTED, OK, RAISED, SURFACE, TEXT, WARN,
};
use crate::util::{self, files, format_size, group, plural};
use crate::widgets::{self, Check};

/// Сколько висит всплывающее сообщение.
const TOAST_TIME: Duration = Duration::from_secs(5);

enum Msg {
    Progress {
        done: usize,
        total: usize,
        current: String,
    },
    Scanned(Option<Plan>),
    Finished(Report),
}

#[derive(Clone, Copy, PartialEq)]
enum JobKind {
    Scan,
    Sort,
    Undo,
}

/// Фоновая работа: сканирование, сортировка или отмена.
struct Job {
    kind: JobKind,
    /// Папка, которую предложим открыть, когда операция закончится.
    folder: Option<PathBuf>,
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
    done: usize,
    total: usize,
    current: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Preview,
    Log,
}

struct CategoryStat {
    name: String,
    files: usize,
    bytes: u64,
    selected: usize,
}

struct Toast {
    text: String,
    error: bool,
    since: Instant,
}

pub struct App {
    paths: Paths,
    exe: Option<PathBuf>,
    logo: egui::TextureHandle,
    classifier: Arc<Classifier>,
    /// Значок и цвет каждой категории, по имени её папки.
    looks: HashMap<String, Look>,
    config_error: Option<String>,
    /// Растёт при перечитывании конфига, чтобы план построился заново.
    config_rev: u64,

    settings: Settings,
    saved_settings: Settings,
    // Текст полей ввода применяется, когда поле теряет фокус
    folder_text: String,
    excluded_text: String,
    settings_open: bool,

    plan: Option<Plan>,
    /// Настройки, для которых построен (или строится) план.
    plan_key: Option<(ScanOptions, u64)>,
    /// Почему плана нет: не выбрана папка, опасная папка и т.п.
    problem: Option<String>,
    off_categories: HashSet<String>,
    filter: Option<String>,
    search: String,
    /// Индексы строк плана, которые видны в таблице.
    view: Vec<usize>,
    stats: Vec<CategoryStat>,
    total_bytes: u64,
    selected: (usize, u64),
    /// Сколько разных папок назначения у отмеченных файлов.
    target_dirs: usize,
    dirty: bool,

    job: Option<Job>,
    last: Option<(PathBuf, Journal)>,
    report: Option<Report>,
    /// Баннер с итогом операции; внутри — папка, которую можно открыть.
    banner: Option<PathBuf>,
    errors_only: bool,
    tab: Tab,
    toast: Option<Toast>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        theme::install(&cc.egui_ctx);
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([64, 64], &crate::logo::rgba(64)),
            egui::TextureOptions::LINEAR,
        );
        let paths = Paths::detect();
        let (config, config_error) = Config::load_or_create(&paths.config);
        let mut settings = Settings::load(&paths.settings);
        let saved_settings = settings.clone();
        if let Some(dir) = initial.filter(|p| p.is_dir()) {
            settings.folder = dir.display().to_string();
        }
        let last = history::last_active(&paths.history);
        Self {
            exe: std::env::current_exe().ok(),
            logo,
            classifier: Arc::new(Classifier::new(&config)),
            looks: category_looks(&config),
            config_error,
            config_rev: 0,
            folder_text: settings.folder.clone(),
            excluded_text: settings.excluded.join("\n"),
            saved_settings,
            settings,
            settings_open: false,
            paths,
            plan: None,
            plan_key: None,
            problem: None,
            off_categories: HashSet::new(),
            filter: None,
            search: String::new(),
            view: Vec::new(),
            stats: Vec::new(),
            total_bytes: 0,
            selected: (0, 0),
            target_dirs: 0,
            dirty: false,
            job: None,
            last,
            report: None,
            banner: None,
            errors_only: false,
            tab: Tab::Preview,
            toast: None,
        }
    }

    /// Идёт сортировка или отмена — настройки трогать нельзя.
    fn busy(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| job.kind != JobKind::Scan)
    }

    fn scanning(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| job.kind == JobKind::Scan)
    }

    fn is_selected(&self, planned: &PlannedMove) -> bool {
        planned.enabled && !self.off_categories.contains(&planned.category)
    }

    fn look(&self, category: &str) -> Look {
        self.looks.get(category).copied().unwrap_or(icons::UNKNOWN)
    }

    fn notify(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            error,
            since: Instant::now(),
        });
    }

    fn scan_options(&self) -> Result<ScanOptions, String> {
        let s = &self.settings;
        if s.folder.is_empty() {
            return Err("Выберите папку".into());
        }
        let root = PathBuf::from(&s.folder);
        if !root.is_absolute() {
            return Err("Укажите полный путь к папке".into());
        }
        if !root.is_dir() {
            return Err(format!("Папка не найдена: {}", root.display()));
        }
        if let Some(reason) = util::danger_reason(&root, s.recursive) {
            return Err(reason.into());
        }
        let output = if s.use_output {
            if s.output.is_empty() {
                return Err("Выберите папку назначения внизу окна".into());
            }
            let output = PathBuf::from(&s.output);
            if !output.is_absolute() {
                return Err("Папка назначения должна быть полным путём".into());
            }
            if let Some(reason) = util::danger_reason(&output, false) {
                return Err(reason.into());
            }
            output
        } else {
            root.clone()
        };
        let mut protected = vec![
            self.paths.config.clone(),
            self.paths.settings.clone(),
            self.paths.history.clone(),
        ];
        protected.extend(self.exe.clone());
        Ok(ScanOptions {
            root,
            output,
            recursive: s.recursive,
            type_folders: s.type_folders,
            skip_sorted: s.skip_sorted,
            skip_hidden: s.skip_hidden,
            detect_content: s.detect_content,
            copy: s.mode == Mode::Copy,
            excluded: s.excluded.clone(),
            protected,
        })
    }

    /// Пересканирует папку, если изменились настройки.
    fn maybe_rescan(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        match self.scan_options() {
            Err(problem) => {
                if let Some(job) = self.job.take() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
                if self.plan.take().is_some() {
                    self.dirty = true;
                }
                self.plan_key = None;
                self.problem = Some(problem);
            }
            Ok(opts) => {
                let key = (opts, self.config_rev);
                if self.plan_key.as_ref() != Some(&key) {
                    self.start_scan(ctx, key);
                }
            }
        }
    }

    fn start_scan(&mut self, ctx: &egui::Context, key: (ScanOptions, u64)) {
        if let Some(old) = self.job.take() {
            old.cancel.store(true, Ordering::Relaxed);
        }
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let classifier = Arc::clone(&self.classifier);
        let opts = key.0.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut last = Instant::now();
            let plan = scanner::scan(&opts, &classifier, &flag, &mut |seen| {
                if last.elapsed() >= Duration::from_millis(50) {
                    last = Instant::now();
                    let _ = tx.send(Msg::Progress {
                        done: seen,
                        total: 0,
                        current: String::new(),
                    });
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Msg::Scanned(plan));
            ctx.request_repaint();
        });
        self.job = Some(Job {
            kind: JobKind::Scan,
            folder: None,
            rx,
            cancel,
            done: 0,
            total: 0,
            current: String::new(),
        });
        self.plan_key = Some(key);
        self.problem = None;
    }

    fn start_op(
        &mut self,
        ctx: &egui::Context,
        kind: JobKind,
        folder: PathBuf,
        work: impl FnOnce(&AtomicBool, Progress<'_>) -> Report + Send + 'static,
    ) {
        if let Some(old) = self.job.take() {
            old.cancel.store(true, Ordering::Relaxed);
        }
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut last: Option<Instant> = None;
            let report = work(&flag, &mut |done, total, current| {
                if done == total || last.is_none_or(|t| t.elapsed() >= Duration::from_millis(40)) {
                    last = Some(Instant::now());
                    let _ = tx.send(Msg::Progress {
                        done,
                        total,
                        current: current.to_owned(),
                    });
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Msg::Finished(report));
            ctx.request_repaint();
        });
        self.job = Some(Job {
            kind,
            folder: Some(folder),
            rx,
            cancel,
            done: 0,
            total: 0,
            current: String::new(),
        });
        self.banner = None;
    }

    fn start_sort(&mut self, ctx: &egui::Context) {
        let Some(plan) = &self.plan else { return };
        let moves: Vec<PlannedMove> = plan
            .moves
            .iter()
            .filter(|m| self.is_selected(m))
            .cloned()
            .collect();
        if moves.is_empty() {
            return;
        }
        let s = &self.settings;
        let output = plan.output.clone();
        let job = SortJob {
            source: plan.root.clone(),
            output: output.clone(),
            mode: s.mode,
            remove_empty: s.remove_empty && s.recursive && s.mode == Mode::Move,
            moves,
            journal_path: history::new_journal_path(&self.paths.history),
        };
        let history_dir = self.paths.history.clone();
        self.start_op(ctx, JobKind::Sort, output, move |cancel, progress| {
            let report = sorter::run_sort(job, cancel, progress);
            history::prune(&history_dir, 100);
            report
        });
    }

    fn start_undo(&mut self, ctx: &egui::Context) {
        let Some((path, journal)) = self.last.take() else {
            return;
        };
        let folder = journal.source.clone();
        self.start_op(ctx, JobKind::Undo, folder, move |cancel, progress| {
            sorter::run_undo(&path, journal, cancel, progress)
        });
    }

    fn poll_job(&mut self) {
        let Some(job) = self.job.as_mut() else { return };
        let finished = loop {
            match job.rx.try_recv() {
                Ok(Msg::Progress {
                    done,
                    total,
                    current,
                }) => {
                    job.done = done;
                    job.total = total;
                    job.current = current;
                }
                Ok(msg) => break msg,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.notify("Операция прервалась из-за внутренней ошибки", true);
                    self.last = history::last_active(&self.paths.history);
                    self.plan_key = None;
                    return;
                }
            }
        };
        let folder = self.job.take().and_then(|job| job.folder);
        match finished {
            Msg::Scanned(Some(plan)) => {
                if let Some(filter) = &self.filter
                    && !plan.moves.iter().any(|m| &m.category == filter)
                {
                    self.filter = None;
                }
                self.plan = Some(plan);
                self.dirty = true;
            }
            Msg::Finished(report) => {
                self.banner = folder;
                self.errors_only = false;
                self.report = Some(report);
                self.last = history::last_active(&self.paths.history);
                // Файлы переехали — строим план заново
                self.plan_key = None;
            }
            Msg::Scanned(None) | Msg::Progress { .. } => {}
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        let Some(path) = dropped else { return };
        if self.busy() {
            self.notify("Дождитесь окончания текущей операции", true);
        } else if path.is_dir() {
            self.set_folder(path);
        } else {
            self.notify("Перетащите папку, а не файл", true);
        }
    }

    fn set_folder(&mut self, path: PathBuf) {
        self.folder_text = path.display().to_string();
        self.commit_folder_text();
        self.tab = Tab::Preview;
    }

    fn commit_folder_text(&mut self) {
        // «Копировать как путь» в проводнике добавляет кавычки
        self.folder_text = self.folder_text.trim().trim_matches('"').trim().to_string();
        if self.folder_text != self.settings.folder {
            self.settings.folder = self.folder_text.clone();
            self.filter = None;
            self.search.clear();
            self.banner = None;
            self.dirty = true;
        }
    }

    fn commit_excluded(&mut self) {
        self.settings.excluded = self
            .excluded_text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect();
    }

    fn browse_folder(&mut self) {
        if let Some(dir) = pick_folder("Папка для сортировки", &self.settings.folder)
        {
            self.set_folder(dir);
        }
    }

    fn reload_config(&mut self) {
        let (config, error) = Config::load_or_create(&self.paths.config);
        self.classifier = Arc::new(Classifier::new(&config));
        self.looks = category_looks(&config);
        match &error {
            Some(e) => self.notify(e.clone(), true),
            None => self.notify("Категории перечитаны", false),
        }
        self.config_error = error;
        self.config_rev += 1;
    }

    fn open_config(&mut self) {
        if !self.paths.config.exists()
            && let Err(e) = Config::default().save(&self.paths.config)
        {
            self.notify(format!("Не удалось создать файл категорий: {e}"), true);
            return;
        }
        if let Err(e) = util::open_path(&self.paths.config) {
            self.notify(format!("Не удалось открыть файл: {e}"), true);
        }
    }

    fn save_settings(&mut self) {
        if self.settings == self.saved_settings {
            return;
        }
        if let Err(e) = self.settings.save(&self.paths.settings) {
            self.notify(format!("Настройки не сохранились: {e}"), true);
        }
        self.saved_settings = self.settings.clone();
    }

    /// Пересчитывает счётчики категорий и список видимых строк.
    fn refresh_view(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        self.view.clear();
        self.stats.clear();
        self.total_bytes = 0;
        self.selected = (0, 0);
        self.target_dirs = 0;
        let Some(plan) = &self.plan else { return };

        let needle = self.search.trim().to_lowercase();
        let mut dirs = HashSet::new();
        for (index, planned) in plan.moves.iter().enumerate() {
            let selected = self.is_selected(planned);
            let position = match self.stats.iter().position(|s| s.name == planned.category) {
                Some(position) => position,
                None => {
                    self.stats.push(CategoryStat {
                        name: planned.category.clone(),
                        files: 0,
                        bytes: 0,
                        selected: 0,
                    });
                    self.stats.len() - 1
                }
            };
            let stat = &mut self.stats[position];
            stat.files += 1;
            stat.bytes += planned.size;
            self.total_bytes += planned.size;
            if selected {
                stat.selected += 1;
                self.selected.0 += 1;
                self.selected.1 += planned.size;
                if let Some(dir) = planned.dst.parent() {
                    dirs.insert(util::path_key(dir));
                }
            }

            let category_ok = self.filter.as_ref().is_none_or(|f| *f == planned.category);
            let search_ok = needle.is_empty()
                || planned
                    .src
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(&needle));
            if category_ok && search_ok {
                self.view.push(index);
            }
        }
        self.target_dirs = dirs.len();
        // Большие категории выше, неопознанные файлы всегда в конце
        let unknown = self.classifier.unknown();
        self.stats.sort_by(|a, b| {
            (a.name == unknown)
                .cmp(&(b.name == unknown))
                .then(b.files.cmp(&a.files))
        });
    }

    // ---------- шапка ----------

    fn header(&mut self, ui: &mut Ui) {
        let enabled = !self.busy();
        let mut undo = false;
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(&self.logo).fit_to_exact_size(vec2(28.0, 28.0)));
            ui.label(
                RichText::new("MH Sort")
                    .font(theme::bold_font(17.0))
                    .color(TEXT),
            );
            ui.add_space(20.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let settings = Button::selectable(
                    self.settings_open,
                    format!("{}  Настройки", ph::regular::GEAR_SIX),
                );
                if ui.add(settings).clicked() {
                    self.settings_open = !self.settings_open;
                }
                if self.banner.is_none()
                    && self.job.is_none()
                    && let Some((_, journal)) = &self.last
                {
                    let button = Button::new(format!(
                        "{}  Отменить последнюю",
                        ph::regular::ARROW_COUNTER_CLOCKWISE
                    ))
                    .frame_when_inactive(false);
                    undo = ui
                        .add(button)
                        .on_hover_text(last_operation(journal))
                        .clicked();
                }
                ui.add_space(14.0);
                let rescan = ui
                    .add_enabled(
                        enabled && self.plan_key.is_some(),
                        widgets::icon_button(ph::regular::ARROWS_CLOCKWISE),
                    )
                    .on_hover_text("Проверить папку заново");
                if rescan.clicked() {
                    self.plan_key = None;
                }
                ui.add_enabled_ui(enabled, |ui| {
                    widgets::switch_inline(ui, &mut self.settings.recursive, "С подпапками")
                        .on_hover_text("Брать файлы и из вложенных папок");
                });
                ui.add_space(6.0);
                let browse = Button::new(format!("{}  Обзор…", ph::regular::FOLDER_OPEN));
                if ui.add_enabled(enabled, browse).clicked() {
                    self.browse_folder();
                }
                let field = TextEdit::singleline(&mut self.folder_text)
                    .hint_text("Папка, которую нужно разобрать")
                    .desired_width(f32::INFINITY)
                    .margin(Margin {
                        left: 32,
                        right: 8,
                        top: 6,
                        bottom: 6,
                    });
                let response = ui.add_enabled(enabled, field);
                ui.painter().text(
                    pos2(response.rect.left() + 16.0, response.rect.center().y),
                    Align2::CENTER_CENTER,
                    ph::regular::FOLDER_SIMPLE,
                    FontId::proportional(15.0),
                    MUTED,
                );
                if response.lost_focus() {
                    self.commit_folder_text();
                }
            });
        });
        if undo {
            let ctx = ui.ctx().clone();
            self.start_undo(&ctx);
        }
    }

    // ---------- подвал ----------

    fn footer(&mut self, ui: &mut Ui) {
        if let Some(job) = &self.job
            && job.kind != JobKind::Scan
        {
            let mut stop = false;
            egui::Sides::new().height(38.0).show(
                ui,
                |ui| {
                    let fraction = if job.total > 0 {
                        job.done as f32 / job.total as f32
                    } else {
                        0.0
                    };
                    let (track, _) = ui.allocate_exact_size(vec2(300.0, 8.0), Sense::hover());
                    ui.painter().rect_filled(track, 4.0, RAISED);
                    let done = Rect::from_min_size(
                        track.min,
                        vec2(track.width() * fraction, track.height()),
                    );
                    ui.painter().rect_filled(done, 4.0, MOON);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!("{} из {}", group(job.done), group(job.total)))
                            .color(TEXT),
                    );
                    ui.add(egui::Label::new(RichText::new(&job.current).color(MUTED)).truncate());
                },
                |ui| {
                    let text = format!("{}  Остановить", ph::regular::STOP);
                    stop = ui
                        .add(Button::new(text).min_size(vec2(150.0, 38.0)))
                        .clicked();
                },
            );
            if stop {
                job.cancel.store(true, Ordering::Relaxed);
            }
            return;
        }

        let (count, _) = self.selected;
        let ready = count > 0 && self.job.is_none() && self.plan.is_some();
        let label = match (self.plan.is_some() && count > 0, self.settings.mode) {
            (false, _) => "Нечего раскладывать".to_string(),
            (true, Mode::Move) => format!("Разложить {}", files(count)),
            (true, Mode::Copy) => format!("Скопировать {}", files(count)),
        };
        let mut sort = false;
        egui::Sides::new().height(38.0).show(
            ui,
            |ui| {
                widgets::segmented(
                    ui,
                    &mut self.settings.mode,
                    &[(Mode::Move, "Переместить"), (Mode::Copy, "Копировать")],
                );
                ui.add_space(16.0);
                ui.label(RichText::new("Куда:").color(MUTED));
                self.destination_menu(ui);
            },
            |ui| {
                sort = widgets::primary(ui, &label, ready).clicked();
            },
        );
        if sort {
            let ctx = ui.ctx().clone();
            self.start_sort(&ctx);
        }
    }

    fn destination_menu(&mut self, ui: &mut Ui) {
        let other = self.settings.use_output && !self.settings.output.is_empty();
        let (name, tooltip) = if other {
            let path = Path::new(&self.settings.output);
            (
                folder_name(path),
                format!("Папки категорий появятся в {}", path.display()),
            )
        } else if self.settings.folder.is_empty() {
            (
                "исходная папка".to_string(),
                "Папки категорий появятся внутри исходной папки".to_string(),
            )
        } else {
            (
                folder_name(Path::new(&self.settings.folder)),
                "Папки категорий появятся внутри исходной папки".to_string(),
            )
        };
        let text = format!(
            "{}  {name}   {}",
            ph::regular::FOLDER_SIMPLE,
            ph::regular::CARET_DOWN
        );
        let response = ui
            .add(Button::new(text).min_size(vec2(0.0, 34.0)))
            .on_hover_text(tooltip);
        let mut pick = false;
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(260.0);
            if ui
                .add(Button::selectable(!other, "Исходная папка"))
                .clicked()
            {
                self.settings.use_output = false;
                ui.close();
            }
            if ui.add(Button::selectable(other, "Другая папка…")).clicked() {
                pick = true;
                ui.close();
            }
            if other {
                ui.label(RichText::new(&self.settings.output).size(12.5).color(MUTED));
            }
        });
        if pick
            && let Some(dir) =
                pick_folder("Куда складывать разложенные файлы", &self.settings.output)
        {
            self.settings.output = dir.display().to_string();
            self.settings.use_output = true;
        }
    }

    // ---------- настройки ----------

    /// Возвращает `true`, если панель попросили закрыть.
    fn settings_drawer(&mut self, ui: &mut Ui) -> bool {
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Настройки")
                    .font(theme::bold_font(17.0))
                    .color(TEXT),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = ui
                    .add(widgets::icon_button(ph::regular::X))
                    .on_hover_text("Закрыть")
                    .clicked();
            });
        });
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_enabled_ui(!self.busy(), |ui| {
                    widgets::section(ui, "Раскладка");
                    widgets::switch(
                        ui,
                        &mut self.settings.type_folders,
                        "Папки по типам внутри категорий",
                        Some("Видео\\MP4, Видео\\MKV. Без этого файлы лягут прямо в папку категории."),
                    );
                    widgets::switch(
                        ui,
                        &mut self.settings.detect_content,
                        "Узнавать тип по содержимому",
                        Some("Для файлов без расширения или с незнакомым расширением."),
                    );

                    widgets::section(ui, "Что пропускать");
                    widgets::switch(
                        ui,
                        &mut self.settings.skip_hidden,
                        "Скрытые и системные файлы",
                        None,
                    );
                    let nested = self.settings.recursive && !self.settings.use_output;
                    ui.add_enabled_ui(nested, |ui| {
                        widgets::switch(
                            ui,
                            &mut self.settings.skip_sorted,
                            "Уже разложенные папки",
                            Some("Папки категорий внутри исходной. Работает вместе с «С подпапками»."),
                        );
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Исключённые папки").color(TEXT));
                    widgets::hint(ui, "Имена папок, по одному в строке. Внутрь них программа не заходит.");
                    let edit = TextEdit::multiline(&mut self.excluded_text)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("node_modules");
                    if ui.add(edit).lost_focus() {
                        self.commit_excluded();
                    }

                    widgets::section(ui, "После перемещения");
                    let can_remove = self.settings.recursive && self.settings.mode == Mode::Move;
                    ui.add_enabled_ui(can_remove, |ui| {
                        widgets::switch(
                            ui,
                            &mut self.settings.remove_empty,
                            "Удалять опустевшие папки",
                            Some("Только те, что опустели после перемещения. Работает вместе с «С подпапками»."),
                        );
                    });

                    widgets::section(ui, "Категории");
                    self.config_section(ui);
                });
            });
        close
    }

    fn config_section(&mut self, ui: &mut Ui) {
        let categories = self.classifier.category_count();
        let extensions = self.classifier.extension_count;
        ui.label(
            RichText::new(format!(
                "{categories} {}, {extensions} {}",
                plural(categories, "категория", "категории", "категорий"),
                plural(extensions, "расширение", "расширения", "расширений"),
            ))
            .color(MUTED),
        );
        if let Some(error) = &self.config_error {
            ui.colored_label(ERROR, error);
        }
        for warning in &self.classifier.warnings {
            ui.colored_label(WARN, warning);
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Открыть categories.json").clicked() {
                self.open_config();
            }
            if ui
                .button("Перечитать")
                .on_hover_text("Применить изменения из файла")
                .clicked()
            {
                self.reload_config();
            }
        });
        widgets::hint(ui, &self.paths.config.display().to_string());

        widgets::section(ui, "Журналы");
        widgets::hint(
            ui,
            "По журналам работает отмена. Хранятся последние 100 операций.",
        );
        ui.add_space(2.0);
        let open = format!("{}  Открыть папку журналов", ph::regular::ARROW_SQUARE_OUT);
        if ui.button(open).clicked() {
            let _ = std::fs::create_dir_all(&self.paths.history);
            if let Err(e) = util::open_path(&self.paths.history) {
                self.notify(format!("Не удалось открыть папку: {e}"), true);
            }
        }
    }

    // ---------- рабочая область ----------

    fn main_area(&mut self, ui: &mut Ui) {
        if self.banner.is_some() {
            self.banner_ui(ui);
            ui.add_space(14.0);
        }
        if self.plan.is_none() {
            if self.scanning() {
                self.scanning_state(ui);
            } else {
                self.start_state(ui);
            }
            return;
        }
        self.overview(ui);
        self.tabs_row(ui);
        ui.add_space(8.0);
        match self.tab {
            Tab::Preview => self.preview(ui),
            Tab::Log => self.log_view(ui),
        }
    }

    fn banner_ui(&mut self, ui: &mut Ui) {
        let (Some(report), Some(folder)) = (&self.report, &self.banner) else {
            return;
        };
        let (icon, color) = if report.failed > 0 {
            (ph::fill::WARNING_CIRCLE, ERROR)
        } else if report.cancelled || report.note.is_some() {
            (ph::fill::WARNING_CIRCLE, WARN)
        } else {
            (ph::fill::CHECK_CIRCLE, OK)
        };
        let title = outcome_title(report);
        let detail = outcome_detail(report, folder);
        let note = report.note.clone();
        let failed = report.failed;
        // Отменять можно, только если журнал этой операции сохранился
        let can_undo = matches!(report.action, Action::Sort(_))
            && report.done > 0
            && report.note.is_none()
            && self.last.is_some()
            && self.job.is_none();
        let folder = folder.clone();
        let (mut open, mut log, mut undo, mut close) = (false, false, false, false);

        let frame = Frame::new()
            .fill(RAISED)
            .corner_radius(10)
            .inner_margin(Margin {
                left: 20,
                right: 12,
                top: 12,
                bottom: 12,
            });
        let rect = frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.set_min_height(44.0);
                    ui.label(
                        RichText::new(icon)
                            .family(icons::fill())
                            .size(26.0)
                            .color(color),
                    );
                    ui.add_space(4.0);
                    // Сначала кнопки справа, текст получает оставшуюся ширину
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        close = ui
                            .add(widgets::icon_button(ph::regular::X))
                            .on_hover_text("Скрыть")
                            .clicked();
                        if can_undo {
                            let text =
                                format!("{}  Отменить", ph::regular::ARROW_COUNTER_CLOCKWISE);
                            undo = ui.button(text).clicked();
                        }
                        log = ui
                            .button(if failed > 0 {
                                "Показать ошибки"
                            } else {
                                "Журнал"
                            })
                            .clicked();
                        let text = format!("{}  Открыть папку", ph::regular::FOLDER_OPEN);
                        open = ui.button(text).clicked();
                        ui.add_space(12.0);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.label(
                                RichText::new(&title)
                                    .font(theme::bold_font(15.0))
                                    .color(TEXT),
                            );
                            ui.add(
                                egui::Label::new(RichText::new(&detail).color(MUTED)).truncate(),
                            )
                            .on_hover_text(folder.display().to_string());
                            if let Some(note) = &note {
                                ui.add(
                                    egui::Label::new(RichText::new(note).color(WARN)).truncate(),
                                );
                            }
                        });
                    });
                });
            })
            .response
            .rect;
        // Цветная кромка слева говорит об итоге раньше текста
        let edge = Rect::from_min_size(rect.min, vec2(4.0, rect.height()));
        let corners = CornerRadius {
            nw: 10,
            sw: 10,
            ne: 0,
            se: 0,
        };
        ui.painter().rect_filled(edge, corners, color);

        if open && let Err(e) = util::open_path(&folder) {
            self.notify(format!("Не удалось открыть папку: {e}"), true);
        }
        if log {
            self.tab = Tab::Log;
            self.errors_only = failed > 0;
        }
        if undo {
            let ctx = ui.ctx().clone();
            self.start_undo(&ctx);
        }
        if close {
            self.banner = None;
        }
    }

    fn overview(&mut self, ui: &mut Ui) {
        let Some(plan) = &self.plan else { return };
        let total = plan.moves.len();
        if total == 0 {
            return;
        }
        let (in_place, ignored) = (plan.in_place, plan.ignored);
        let unread = plan.errors.len();
        let unread_details = plan
            .errors
            .iter()
            .take(15)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let (count, bytes) = self.selected;
        let dirs = self.target_dirs;
        let mode = self.settings.mode;
        let scanning = self.scanning();

        ui.horizontal(|ui| {
            if count == 0 {
                ui.label(
                    RichText::new("Ничего не отмечено")
                        .font(theme::bold_font(19.0))
                        .color(TEXT),
                );
                ui.label(RichText::new("отметьте файлы или категории ниже").color(MUTED));
            } else {
                ui.label(
                    RichText::new(files(count))
                        .font(theme::bold_font(19.0))
                        .color(TEXT),
                );
                if count < total {
                    ui.label(RichText::new(format!("из {}", group(total))).color(MUTED));
                }
                let phrase = match mode {
                    Mode::Move => format!(
                        "{} по {} {}",
                        plural(count, "разложится", "разложатся", "разложатся"),
                        group(dirs),
                        plural(dirs, "папке", "папкам", "папкам"),
                    ),
                    Mode::Copy => format!(
                        "{} в {} {}",
                        plural(count, "скопируется", "скопируются", "скопируются"),
                        group(dirs),
                        plural(dirs, "папку", "папки", "папок"),
                    ),
                };
                ui.label(RichText::new(phrase).color(TEXT));
                ui.label(RichText::new(format_size(bytes)).color(MUTED));
            }
            if scanning {
                ui.add_space(6.0);
                ui.add(egui::Spinner::new().size(14.0).color(MUTED));
                ui.label(RichText::new("обновляю").color(MUTED));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if unread > 0 {
                    widgets::badge(
                        ui,
                        ph::regular::WARNING,
                        &format!("Не прочитано: {unread}"),
                        WARN,
                    )
                    .on_hover_text(&unread_details);
                }
                if ignored > 0 {
                    widgets::badge(
                        ui,
                        ph::regular::EYE_SLASH,
                        &format!("Пропущено: {}", group(ignored)),
                        MUTED,
                    )
                    .on_hover_text("Скрытые, системные и недокачанные файлы");
                }
                if in_place > 0 {
                    widgets::badge(
                        ui,
                        ph::regular::CHECKS,
                        &format!("На месте: {}", group(in_place)),
                        OK,
                    )
                    .on_hover_text("Уже лежат в своих папках");
                }
            });
        });
        ui.add_space(10.0);
        if let Some(filter) = self.composition_bar(ui) {
            self.filter = filter;
            self.dirty = true;
        }
        ui.add_space(16.0);
    }

    /// Полоса состава папки: по отрезку на категорию. Клик по отрезку — фильтр.
    fn composition_bar(&self, ui: &mut Ui) -> Option<Option<String>> {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 12.0), Sense::hover());
        let total: usize = self.stats.iter().map(|s| s.files).sum();
        if total == 0 {
            return None;
        }
        let gap = 3.0;
        let usable = rect.width() - gap * (self.stats.len() - 1) as f32;
        // Мелкие категории всё равно видны
        let widths: Vec<f32> = self
            .stats
            .iter()
            .map(|s| (s.files as f32 / total as f32 * usable).max(6.0))
            .collect();
        let scale = usable / widths.iter().sum::<f32>();
        let mut x = rect.left();
        let mut clicked = None;
        for (stat, width) in self.stats.iter().zip(widths) {
            let segment =
                Rect::from_min_size(pos2(x, rect.top()), vec2(width * scale, rect.height()));
            x += width * scale + gap;
            let response = ui.interact(segment, Id::new(("segment", &stat.name)), Sense::click());
            let mut color = self.look(&stat.name).color;
            if self.off_categories.contains(&stat.name) {
                color = color.gamma_multiply(0.3);
            }
            if self.filter.as_ref().is_some_and(|f| *f != stat.name) {
                color = color.gamma_multiply(0.35);
            }
            let shown = if response.hovered() {
                segment.expand2(vec2(0.0, 2.0))
            } else {
                segment
            };
            ui.painter().rect_filled(shown, 4.0, color);
            let text = format!(
                "{}: {}, {}",
                stat.name,
                files(stat.files),
                format_size(stat.bytes)
            );
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &text));
            if response.on_hover_text(&text).clicked() {
                let same = self.filter.as_deref() == Some(stat.name.as_str());
                clicked = Some((!same).then(|| stat.name.clone()));
            }
        }
        clicked
    }

    fn tabs_row(&mut self, ui: &mut Ui) {
        let errors = self.report.as_ref().map_or(0, |r| r.failed);
        let mut set_all = None;
        let row = ui.horizontal(|ui| {
            if widgets::tab(ui, self.tab == Tab::Preview, "Предпросмотр", None).clicked()
            {
                self.tab = Tab::Preview;
            }
            let badge = (errors > 0).then_some(errors);
            if widgets::tab(ui, self.tab == Tab::Log, "Журнал", badge).clicked() {
                self.tab = Tab::Log;
            }
            let has_files = self.plan.as_ref().is_some_and(|p| !p.moves.is_empty());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| match self.tab {
                Tab::Preview if !has_files => {}
                Tab::Preview => {
                    let none = Button::new(format!("{}  Снять все", ph::regular::SQUARE))
                        .frame_when_inactive(false);
                    if ui.add(none).on_hover_text("Для показанных строк").clicked()
                    {
                        set_all = Some(false);
                    }
                    let all = Button::new(format!("{}  Отметить все", ph::regular::CHECK_SQUARE))
                        .frame_when_inactive(false);
                    if ui.add(all).on_hover_text("Для показанных строк").clicked()
                    {
                        set_all = Some(true);
                    }
                    ui.add_space(6.0);
                    let search = TextEdit::singleline(&mut self.search)
                        .hint_text("Поиск по имени")
                        .desired_width(230.0)
                        .margin(Margin {
                            left: 30,
                            right: 8,
                            top: 5,
                            bottom: 5,
                        });
                    let response = ui.add(search);
                    ui.painter().text(
                        pos2(response.rect.left() + 15.0, response.rect.center().y),
                        Align2::CENTER_CENTER,
                        ph::regular::MAGNIFYING_GLASS,
                        FontId::proportional(15.0),
                        MUTED,
                    );
                    if response.changed() {
                        self.dirty = true;
                    }
                }
                Tab::Log => {
                    if self.report.is_some() {
                        widgets::switch_inline(ui, &mut self.errors_only, "Только ошибки");
                    }
                }
            });
        });
        let rect = row.response.rect;
        ui.painter()
            .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, LINE));
        if let Some(value) = set_all {
            if let Some(plan) = self.plan.as_mut() {
                for &index in &self.view {
                    plan.moves[index].enabled = value;
                }
            }
            self.dirty = true;
        }
    }

    fn preview(&mut self, ui: &mut Ui) {
        if self.plan.as_ref().is_some_and(|p| p.moves.is_empty()) {
            self.nothing_state(ui);
            return;
        }
        egui::Panel::left("categories")
            .frame(Frame::new().inner_margin(Margin {
                left: 0,
                right: 12,
                top: 4,
                bottom: 0,
            }))
            .default_size(250.0)
            .size_range(210.0..=380.0)
            .show(ui, |ui| self.category_list(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if self.view.is_empty() {
                self.no_matches_state(ui);
            } else {
                self.file_table(ui);
            }
        });
    }

    fn category_list(&mut self, ui: &mut Ui) {
        let total = self.plan.as_ref().map_or(0, |p| p.moves.len());
        let mut new_filter = None;
        let mut toggle = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let all_look = Look {
                    icon: ph::fill::FOLDERS,
                    color: MOON,
                };
                let (row, _) = widgets::category_row(
                    ui,
                    &all_look,
                    "Все файлы",
                    &group(total),
                    self.filter.is_none(),
                    None,
                );
                if row.on_hover_text(format_size(self.total_bytes)).clicked() {
                    new_filter = Some(None);
                }
                ui.add_space(6.0);
                for stat in &self.stats {
                    let mut on = !self.off_categories.contains(&stat.name);
                    let count = if stat.selected == stat.files || !on {
                        group(stat.files)
                    } else {
                        format!("{} из {}", group(stat.selected), group(stat.files))
                    };
                    let active = self.filter.as_deref() == Some(stat.name.as_str());
                    let look = self.look(&stat.name);
                    let (row, changed) =
                        widgets::category_row(ui, &look, &stat.name, &count, active, Some(&mut on));
                    if changed {
                        toggle = Some((stat.name.clone(), on));
                    }
                    let hover = format!("{}, {}", files(stat.files), format_size(stat.bytes));
                    if row.on_hover_text(hover).clicked() {
                        new_filter = Some((!active).then(|| stat.name.clone()));
                    }
                }
            });
        if let Some(filter) = new_filter {
            self.filter = filter;
            self.dirty = true;
        }
        if let Some((name, on)) = toggle {
            if on {
                self.off_categories.remove(&name);
            } else {
                self.off_categories.insert(name);
            }
            self.dirty = true;
        }
    }

    fn file_table(&mut self, ui: &mut Ui) {
        let Some(plan) = self.plan.as_mut() else {
            return;
        };
        let Plan {
            root,
            output,
            moves,
            ..
        } = plan;
        let (root, output): (&Path, &Path) = (root, output);
        let looks = &self.looks;
        let off = &self.off_categories;
        let view = &self.view;
        let mut changed = false;
        let mut reveal = None;
        TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
            .auto_shrink([false, false])
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::exact(28.0).resizable(false))
            .column(Column::remainder().at_least(150.0).clip(true))
            .column(Column::exact(96.0).resizable(false))
            .column(Column::remainder().at_least(170.0).clip(true))
            .header(28.0, |mut header| {
                header.col(|_| {});
                header.col(|ui| column_title(ui, "Файл"));
                header.col(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(16.0);
                        column_title(ui, "Размер");
                    });
                });
                header.col(|ui| column_title(ui, "Куда"));
            })
            .body(|body| {
                body.rows(32.0, view.len(), |mut row| {
                    let planned = &mut moves[view[row.index()]];
                    let look = looks
                        .get(&planned.category)
                        .copied()
                        .unwrap_or(icons::UNKNOWN);
                    let category_on = !off.contains(&planned.category);
                    let active = category_on && planned.enabled;
                    let name = planned
                        .src
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    row.col(|ui| {
                        let check =
                            ui.add_enabled(category_on, Check::new(&mut planned.enabled, &name));
                        if check.changed() {
                            changed = true;
                        }
                    });
                    row.col(|ui| {
                        let mut job = LayoutJob::default();
                        let color = if active { TEXT } else { FAINT };
                        job.append(
                            &name,
                            0.0,
                            TextFormat::simple(FontId::proportional(14.0), color),
                        );
                        let subfolder = planned
                            .src
                            .parent()
                            .and_then(|p| p.strip_prefix(root).ok())
                            .filter(|p| !p.as_os_str().is_empty());
                        if let Some(sub) = subfolder {
                            let format = TextFormat::simple(
                                FontId::proportional(12.5),
                                if active { MUTED } else { FAINT },
                            );
                            job.append(&sub.display().to_string(), 10.0, format);
                        }
                        let label = egui::Label::new(job).truncate().sense(Sense::click());
                        ui.add(label).context_menu(|ui| {
                            let text =
                                format!("{}  Показать в проводнике", ph::regular::ARROW_SQUARE_OUT);
                            if ui.button(text).clicked() {
                                reveal = Some(planned.src.clone());
                                ui.close();
                            }
                        });
                    });
                    row.col(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(16.0);
                            let color = if active { MUTED } else { FAINT };
                            ui.label(RichText::new(format_size(planned.size)).color(color));
                        });
                    });
                    row.col(|ui| {
                        let (chip, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                        widgets::paint_icon_chip(ui.painter(), chip, &look, !active);
                        let place = planned
                            .dst
                            .parent()
                            .and_then(|p| p.strip_prefix(output).ok())
                            .map(|p| {
                                p.components()
                                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                                    .collect::<Vec<_>>()
                                    .join(" › ")
                            })
                            .unwrap_or_default();
                        let mut job = LayoutJob::default();
                        let color = if active { TEXT } else { FAINT };
                        job.append(
                            &place,
                            0.0,
                            TextFormat::simple(FontId::proportional(14.0), color),
                        );
                        let renamed = planned.dst.file_name() != planned.src.file_name();
                        if renamed {
                            let new_name = planned
                                .dst
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy();
                            let format = TextFormat::simple(
                                FontId::proportional(12.5),
                                if active { WARN } else { FAINT },
                            );
                            job.append(&format!("станет {new_name}"), 10.0, format);
                        }
                        let response = ui.add(egui::Label::new(job).truncate());
                        if renamed {
                            response.on_hover_text(
                                "Там уже есть файл с таким именем, поэтому этот получит номер",
                            );
                        }
                        if planned.by_content {
                            ui.label(RichText::new(ph::regular::SPARKLE).color(MUTED))
                                .on_hover_text("Тип определён по содержимому файла");
                        }
                    });
                });
            });
        if changed {
            self.dirty = true;
        }
        if let Some(path) = reveal {
            util::reveal(&path);
        }
    }

    fn log_view(&mut self, ui: &mut Ui) {
        let Some(report) = &self.report else {
            empty_note(ui, "Журнал появится после первой операции.");
            return;
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(outcome_title(report))
                    .font(theme::bold_font(15.0))
                    .color(TEXT),
            );
            if report.failed > 0 {
                ui.label(
                    RichText::new(format!("не удалось: {}", group(report.failed))).color(ERROR),
                );
            }
        });
        ui.add_space(6.0);
        let lines: Vec<_> = report
            .lines
            .iter()
            .filter(|line| !self.errors_only || !line.ok)
            .collect();
        if lines.is_empty() {
            empty_note(ui, "Ошибок нет.");
            return;
        }
        let row_height = 28.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_height, lines.len(), |ui, range| {
                for line in &lines[range] {
                    ui.horizontal(|ui| {
                        ui.set_min_height(row_height);
                        let (icon, color) = if line.ok {
                            (ph::regular::CHECK, OK)
                        } else {
                            (ph::regular::X, ERROR)
                        };
                        ui.label(RichText::new(icon).color(color));
                        let text_color = if line.ok { TEXT } else { ERROR };
                        ui.add(
                            egui::Label::new(RichText::new(&line.text).color(text_color))
                                .truncate(),
                        );
                    });
                }
            });
    }

    // ---------- пустые состояния ----------

    /// Папка не выбрана или её нельзя открыть: зона для перетаскивания.
    fn start_state(&mut self, ui: &mut Ui) {
        let no_folder = self.settings.folder.is_empty();
        // Папку блокирует только режим с подпапками — подскажем, как это исправить
        let root = Path::new(&self.settings.folder);
        let only_recursion = self.settings.recursive
            && !no_folder
            && util::danger_reason(root, false).is_none()
            && util::danger_reason(root, true).is_some();
        let available = ui.available_rect_before_wrap();
        let height: f32 = if no_folder {
            410.0
        } else if only_recursion {
            320.0
        } else {
            270.0
        };
        let size = vec2(
            available.width().min(640.0),
            height.min(available.height() - 16.0),
        );
        let card = Rect::from_center_size(available.center() - vec2(0.0, 12.0), size);
        let accent = if no_folder { MOON } else { WARN };
        ui.painter()
            .rect_filled(card, 18.0, RAISED.gamma_multiply(0.45));
        widgets::dashed_frame(
            ui.painter(),
            card,
            18.0,
            Stroke::new(1.5, accent.gamma_multiply(0.6)),
        );

        let mut browse = false;
        let mut flat = false;
        let builder = UiBuilder::new()
            .max_rect(card.shrink(28.0))
            .layout(Layout::top_down(Align::Center));
        ui.scope_builder(builder, |ui| {
            ui.add_space(4.0);
            let icon = if no_folder {
                ph::fill::FOLDER_OPEN
            } else {
                ph::fill::WARNING_CIRCLE
            };
            ui.label(
                RichText::new(icon)
                    .family(icons::fill())
                    .size(48.0)
                    .color(accent),
            );
            ui.add_space(6.0);
            let title = match (&self.problem, no_folder) {
                (_, true) | (None, _) => "Перетащите сюда папку".to_string(),
                (Some(problem), false) => problem.clone(),
            };
            ui.label(
                RichText::new(title)
                    .font(theme::bold_font(19.0))
                    .color(TEXT),
            );
            ui.add_space(2.0);
            let hint = if no_folder {
                "или выберите её кнопкой. Файлы разложатся по папкам категорий."
            } else if only_recursion {
                "Без подпапок эту папку разобрать можно."
            } else {
                "Выберите другую папку."
            };
            ui.label(RichText::new(hint).color(MUTED));
            ui.add_space(18.0);
            browse = widgets::primary(ui, "Выбрать папку", !self.busy()).clicked();
            if only_recursion {
                ui.add_space(8.0);
                flat = ui.button("Выключить «С подпапками»").clicked();
            }
            if no_folder {
                ui.add_space(26.0);
                self.examples(ui);
            }
        });
        if browse {
            self.browse_folder();
        }
        if flat {
            self.settings.recursive = false;
        }
    }

    /// Как будут разложены типичные файлы — по текущим категориям.
    fn examples(&self, ui: &mut Ui) {
        for name in ["photo.jpg", "model.blend", "movie.mp4"] {
            let class = self.classifier.classify(Path::new(name), false);
            let look = self.look(&class.category);
            let place = if self.settings.type_folders {
                format!("{} › {}", class.category, class.type_folder)
            } else {
                class.category.clone()
            };
            let (rect, _) = ui.allocate_exact_size(vec2(380.0, 32.0), Sense::hover());
            let painter = ui.painter();
            let y = rect.center().y;
            painter.text(
                pos2(rect.left(), y),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(14.0),
                MUTED,
            );
            painter.text(
                pos2(rect.left() + 136.0, y),
                Align2::LEFT_CENTER,
                ph::regular::ARROW_RIGHT,
                FontId::proportional(15.0),
                FAINT,
            );
            let chip = Rect::from_center_size(pos2(rect.left() + 180.0, y), vec2(24.0, 24.0));
            widgets::paint_icon_chip(painter, chip, &look, false);
            painter.text(
                pos2(chip.right() + 10.0, y),
                Align2::LEFT_CENTER,
                place,
                FontId::proportional(14.0),
                TEXT,
            );
        }
    }

    fn scanning_state(&mut self, ui: &mut Ui) {
        let seen = self.job.as_ref().map_or(0, |job| job.done);
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.3).max(20.0));
            ui.add(egui::Spinner::new().size(30.0).color(MOON));
            ui.add_space(12.0);
            ui.label(
                RichText::new("Смотрю, что лежит в папке")
                    .font(theme::bold_font(17.0))
                    .color(TEXT),
            );
            if seen > 0 {
                ui.label(RichText::new(format!("Найдено файлов: {}", group(seen))).color(MUTED));
            }
        });
    }

    fn nothing_state(&mut self, ui: &mut Ui) {
        let (in_place, ignored) = self
            .plan
            .as_ref()
            .map_or((0, 0), |p| (p.in_place, p.ignored));
        let recursive = self.settings.recursive;
        let mut deeper = false;
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.2).max(20.0));
            ui.label(
                RichText::new(ph::fill::CHECK_CIRCLE)
                    .family(icons::fill())
                    .size(48.0)
                    .color(OK),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new("Здесь уже порядок")
                    .font(theme::bold_font(19.0))
                    .color(TEXT),
            );
            let mut facts = Vec::new();
            if in_place > 0 {
                facts.push(format!("на своих местах {}", files(in_place)));
            }
            if ignored > 0 {
                facts.push(format!("пропущено {}", group(ignored)));
            }
            let text = if facts.is_empty() {
                "В папке нет файлов, которые нужно разложить.".to_string()
            } else {
                format!("Раскладывать нечего: {}.", facts.join(", "))
            };
            ui.label(RichText::new(text).color(MUTED));
            if !recursive {
                ui.add_space(16.0);
                ui.label(RichText::new("Файлы во вложенных папках не проверялись.").color(MUTED));
                ui.add_space(4.0);
                deeper = ui.button("Проверить с подпапками").clicked();
            }
        });
        if deeper {
            self.settings.recursive = true;
        }
    }

    fn no_matches_state(&mut self, ui: &mut Ui) {
        let mut reset = false;
        ui.vertical_centered(|ui| {
            ui.add_space(56.0);
            ui.label(
                RichText::new(ph::regular::MAGNIFYING_GLASS)
                    .size(34.0)
                    .color(FAINT),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Ничего не найдено")
                    .font(theme::bold_font(16.0))
                    .color(TEXT),
            );
            ui.label(RichText::new("Измените запрос или покажите все файлы.").color(MUTED));
            ui.add_space(10.0);
            reset = ui.button("Показать все файлы").clicked();
        });
        if reset {
            self.search.clear();
            self.filter = None;
            self.dirty = true;
        }
    }

    // ---------- поверх окна ----------

    fn toast(&mut self, ctx: &egui::Context) {
        let Some(toast) = &self.toast else { return };
        let Some(left) = TOAST_TIME.checked_sub(toast.since.elapsed()) else {
            self.toast = None;
            return;
        };
        ctx.request_repaint_after(left);
        let (icon, color) = if toast.error {
            (ph::regular::WARNING, WARN)
        } else {
            (ph::regular::INFO, MOON)
        };
        let text = toast.text.clone();
        egui::Area::new(Id::new("toast"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -86.0))
            .interactable(false)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(RAISED)
                    .stroke(Stroke::new(1.0, LINE))
                    .corner_radius(10)
                    .inner_margin(Margin::symmetric(16, 10))
                    .shadow(ui.visuals().popup_shadow)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon).size(16.0).color(color));
                            ui.label(RichText::new(text).color(TEXT));
                        });
                    });
            });
    }

    fn drop_overlay(&self, ctx: &egui::Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let painter =
            ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("drop")));
        let screen = ctx.content_rect();
        painter.rect_filled(screen, 0.0, CHROME.gamma_multiply(0.94));
        let zone = screen.shrink(28.0);
        widgets::dashed_frame(&painter, zone, 18.0, Stroke::new(2.0, MOON));
        painter.text(
            zone.center() - vec2(0.0, 26.0),
            Align2::CENTER_CENTER,
            ph::fill::FOLDER_OPEN,
            FontId::new(56.0, icons::fill()),
            MOON,
        );
        painter.text(
            zone.center() + vec2(0.0, 32.0),
            Align2::CENTER_CENTER,
            "Отпустите, чтобы открыть папку",
            theme::bold_font(20.0),
            TEXT,
        );
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job();
        self.handle_drops(ctx);
        self.maybe_rescan(ctx);
        self.save_settings();
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.refresh_view();
        let ctx = ui.ctx().clone();
        egui::Panel::top("header")
            .frame(
                Frame::new()
                    .fill(CHROME)
                    .inner_margin(Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| self.header(ui));
        egui::Panel::bottom("footer")
            .frame(
                Frame::new()
                    .fill(CHROME)
                    .inner_margin(Margin::symmetric(16, 12)),
            )
            .show(ui, |ui| self.footer(ui));
        let mut open = self.settings_open;
        let drawer = egui::Panel::right("settings")
            .frame(Frame::new().fill(CHROME).inner_margin(Margin {
                left: 18,
                right: 14,
                top: 14,
                bottom: 14,
            }))
            .exact_size(340.0)
            .resizable(false)
            .show_collapsible(ui, &mut open, |ui| self.settings_drawer(ui));
        if drawer.is_some_and(|r| r.inner) {
            open = false;
        }
        self.settings_open = open;
        egui::CentralPanel::default()
            .frame(Frame::new().fill(SURFACE).inner_margin(Margin {
                left: 20,
                right: 20,
                top: 16,
                bottom: 12,
            }))
            .show(ui, |ui| self.main_area(ui));
        self.toast(&ctx);
        self.drop_overlay(&ctx);
        if self.dirty {
            ctx.request_repaint();
        }
    }
}

fn category_looks(config: &Config) -> HashMap<String, Look> {
    let mut looks: HashMap<String, Look> = config
        .categories
        .iter()
        .map(|(name, extensions)| (util::sanitize_name(name), icons::look_for(name, extensions)))
        .collect();
    looks.insert(
        util::sanitize_name(&config.unknown_category),
        icons::UNKNOWN,
    );
    looks
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// «Разложено 22 файла», «Удалено 3 копии».
fn outcome_title(report: &Report) -> String {
    let n = report.done;
    match report.action {
        Action::Sort(Mode::Move) => format!(
            "{} {}",
            plural(n, "Разложен", "Разложено", "Разложено"),
            files(n)
        ),
        Action::Sort(Mode::Copy) => {
            format!(
                "{} {}",
                plural(n, "Скопирован", "Скопировано", "Скопировано"),
                files(n)
            )
        }
        Action::Undo(Mode::Move) => {
            format!(
                "{} на место {}",
                plural(n, "Возвращён", "Возвращено", "Возвращено"),
                files(n)
            )
        }
        Action::Undo(Mode::Copy) => format!(
            "{} {} {}",
            plural(n, "Удалена", "Удалено", "Удалено"),
            group(n),
            plural(n, "копия", "копии", "копий")
        ),
    }
}

fn outcome_detail(report: &Report, folder: &Path) -> String {
    if report.cancelled {
        return format!(
            "Остановлено: обработано {} из {}.",
            group(report.done + report.failed),
            group(report.total)
        );
    }
    if report.failed > 0 {
        return format!(
            "Не удалось: {}. Причины смотрите в журнале.",
            group(report.failed)
        );
    }
    match report.action {
        Action::Sort(_) => format!("Папки категорий лежат в «{}»", folder_name(folder)),
        Action::Undo(Mode::Move) => format!("Файлы вернулись в «{}»", folder_name(folder)),
        Action::Undo(Mode::Copy) => "Оригиналы остались на месте.".to_string(),
    }
}

/// Подсказка к кнопке «Отменить последнюю».
fn last_operation(journal: &Journal) -> String {
    let n = journal.entries.len();
    let verb = match journal.mode {
        Mode::Move => plural(n, "Разложен", "Разложено", "Разложено"),
        Mode::Copy => plural(n, "Скопирован", "Скопировано", "Скопировано"),
    };
    format!(
        "{verb} {} в {}\n{}",
        files(n),
        journal.output.display(),
        journal.created
    )
}

fn column_title(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(theme::bold_font(13.0))
            .color(MUTED),
    );
}

fn empty_note(ui: &mut Ui, text: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| ui.label(RichText::new(text).color(MUTED)));
}

fn pick_folder(title: &str, current: &str) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    if !current.is_empty() && Path::new(current).is_dir() {
        dialog = dialog.set_directory(current);
    }
    dialog.pick_folder()
}
