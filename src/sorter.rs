//! Выполнение плана и отмена по журналу.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::history::{Entry, Journal};
use crate::scanner::PlannedMove;
use crate::util::{exists, is_within, path_key, primary_ext, rel, unique_path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Move,
    Copy,
}

pub struct LogLine {
    pub ok: bool,
    pub text: String,
}

/// Что было сделано.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Sort(Mode),
    Undo(Mode),
}

/// Итог операции для окна.
pub struct Report {
    pub action: Action,
    /// Сколько файлов собирались обработать.
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub cancelled: bool,
    pub lines: Vec<LogLine>,
    /// Важное замечание, например «журнал не сохранился».
    pub note: Option<String>,
}

impl Report {
    fn new(action: Action, total: usize) -> Self {
        Self {
            action,
            total,
            done: 0,
            failed: 0,
            cancelled: false,
            lines: Vec::new(),
            note: None,
        }
    }

    fn ok(&mut self, text: String) {
        self.done += 1;
        self.lines.push(LogLine { ok: true, text });
    }

    fn fail(&mut self, text: String) {
        self.failed += 1;
        self.lines.push(LogLine { ok: false, text });
    }
}

pub type Progress<'a> = &'a mut dyn FnMut(usize, usize, &str);

pub struct SortJob {
    pub source: PathBuf,
    pub output: PathBuf,
    pub mode: Mode,
    pub remove_empty: bool,
    pub moves: Vec<PlannedMove>,
    pub journal_path: PathBuf,
}

pub fn run_sort(job: SortJob, cancel: &AtomicBool, progress: Progress<'_>) -> Report {
    let total = job.moves.len();
    let mut report = Report::new(Action::Sort(job.mode), total);
    let mut journal = Journal::new(&job.source, &job.output, job.mode);
    let mut unsaved = 0;

    for (i, planned) in job.moves.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let name = planned
            .src
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        progress(i, total, &name);
        match sort_one(planned, job.mode, &mut journal.created_dirs) {
            Ok(to) => {
                report.ok(format!(
                    "{}  →  {}",
                    rel(&planned.src, &job.source),
                    rel(&to, &job.output)
                ));
                journal.entries.push(Entry {
                    from: planned.src.clone(),
                    to,
                    size: planned.size,
                });
                unsaved += 1;
                // Журнал пишется по ходу дела: даже при сбое останется, что отменять
                if unsaved >= 200 && journal.save(&job.journal_path).is_ok() {
                    unsaved = 0;
                }
            }
            Err(e) => report.fail(format!("{}: {e}", rel(&planned.src, &job.source))),
        }
    }
    progress(total, total, "");

    if job.remove_empty && job.mode == Mode::Move {
        let sources = journal.entries.iter().map(|e| e.from.as_path());
        journal.removed_dirs = remove_empty_dirs(sources, &job.source);
    }
    if !journal.entries.is_empty()
        && let Err(e) = journal.save(&job.journal_path)
    {
        report.note = Some(format!(
            "Журнал не сохранился ({e}) — отменить эту операцию не получится."
        ));
    }
    report
}

fn sort_one(
    planned: &PlannedMove,
    mode: Mode,
    created_dirs: &mut Vec<PathBuf>,
) -> io::Result<PathBuf> {
    if !exists(&planned.src) {
        return Err(io::Error::new(io::ErrorKind::NotFound, "файла больше нет"));
    }
    let dir = planned
        .dst
        .parent()
        .ok_or_else(|| io::Error::other("неверный путь назначения"))?;
    let name = planned
        .src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("неверное имя файла"))?;
    ensure_dir(dir, created_dirs)?;
    // Имя подбирается заново: за время предпросмотра в папке могли появиться файлы
    let to = unique_path(dir, name, &planned.ext, exists);
    match mode {
        Mode::Move => move_file(&planned.src, &to)?,
        Mode::Copy => copy_file(&planned.src, &to)?,
    }
    Ok(to)
}

/// Создаёт папку и запоминает, каких папок раньше не было.
fn ensure_dir(dir: &Path, created: &mut Vec<PathBuf>) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    let missing: Vec<PathBuf> = dir
        .ancestors()
        .take_while(|d| !exists(d))
        .map(Path::to_path_buf)
        .collect();
    fs::create_dir_all(dir)?;
    created.extend(missing.into_iter().rev());
    Ok(())
}

fn already_exists() -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        "файл назначения уже существует",
    )
}

/// Перемещение без перезаписи. Между дисками — копия и удаление оригинала.
pub fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if exists(to) {
        return Err(already_exists());
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device(&e) => {
            copy_file(from, to)?;
            if let Err(e) = fs::remove_file(from) {
                let _ = fs::remove_file(to);
                return Err(e);
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Копирование без перезаписи.
pub fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    if exists(to) {
        return Err(already_exists());
    }
    if let Err(e) = fs::copy(from, to) {
        let _ = fs::remove_file(to);
        return Err(e);
    }
    // Дата изменения как у оригинала (на Windows fs::copy сохраняет её сам)
    if let Ok(modified) = fs::metadata(from).and_then(|m| m.modified()) {
        let _ = fs::File::options()
            .write(true)
            .open(to)
            .and_then(|f| f.set_modified(modified));
    }
    Ok(())
}

fn is_cross_device(e: &io::Error) -> bool {
    // ERROR_NOT_SAME_DEVICE на Windows, EXDEV на Unix
    let code = if cfg!(windows) { 17 } else { 18 };
    e.raw_os_error() == Some(code)
}

/// Удаляет папки внутри `root`, которые опустели после перемещения. Сам `root` не трогает.
/// `remove_dir` удаляет только пустые папки, так что содержимое пострадать не может.
fn remove_empty_dirs<'a>(sources: impl Iterator<Item = &'a Path>, root: &Path) -> Vec<PathBuf> {
    let root_key = path_key(root);
    let mut dirs: HashMap<String, PathBuf> = HashMap::new();
    for src in sources {
        for dir in src.ancestors().skip(1) {
            let key = path_key(dir);
            if key == root_key || !is_within(dir, root) || dirs.contains_key(&key) {
                break;
            }
            dirs.insert(key, dir.to_path_buf());
        }
    }
    let mut list: Vec<PathBuf> = dirs.into_values().collect();
    // Сначала самые глубокие: родитель пустеет только после детей
    list.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    list.into_iter()
        .filter(|d| fs::remove_dir(d).is_ok())
        .collect()
}

/// Отмена операции из журнала.
pub fn run_undo(
    path: &Path,
    mut journal: Journal,
    cancel: &AtomicBool,
    progress: Progress<'_>,
) -> Report {
    let mut report = Report::new(Action::Undo(journal.mode), journal.entries.len());
    for dir in journal.removed_dirs.iter().rev() {
        let _ = fs::create_dir_all(dir);
    }

    let total = journal.entries.len();
    let mut remaining = total;
    for (step, entry) in journal.entries.iter().rev().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        progress(
            step,
            total,
            &entry.to.file_name().unwrap_or_default().to_string_lossy(),
        );
        let shown = rel(&entry.to, &journal.output);
        let result = match journal.mode {
            Mode::Move => undo_move(entry).map(|back| {
                let note = if back == entry.from {
                    ""
                } else {
                    "  (исходное имя занято)"
                };
                format!("{shown}  →  {}{note}", rel(&back, &journal.source))
            }),
            Mode::Copy => undo_copy(entry).map(|removed| {
                let what = if removed {
                    "копия удалена"
                } else {
                    "копии уже нет"
                };
                format!("{shown}: {what}")
            }),
        };
        match result {
            Ok(text) => report.ok(text),
            Err(e) => report.fail(format!("{shown}: {e}")),
        }
        remaining -= 1;
    }
    progress(total, total, "");

    let mut created = journal.created_dirs.clone();
    created.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for dir in created {
        let _ = fs::remove_dir(dir);
    }

    // Прерванная отмена оставляет в журнале только то, что ещё не вернули
    journal.entries.truncate(remaining);
    journal.undone = journal.entries.is_empty();
    if let Err(e) = journal.save(path) {
        report.note = Some(format!("Не удалось обновить журнал: {e}"));
    }
    report
}

fn undo_move(entry: &Entry) -> io::Result<PathBuf> {
    if !exists(&entry.to) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "файл не найден — его переместили или удалили",
        ));
    }
    let dir = entry
        .from
        .parent()
        .ok_or_else(|| io::Error::other("неверный исходный путь"))?;
    let name = entry
        .from
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("неверное имя"))?;
    fs::create_dir_all(dir)?;
    let back = unique_path(dir, name, &primary_ext(name), exists);
    move_file(&entry.to, &back)?;
    Ok(back)
}

/// Удаляет копию, только если оригинал на месте и копия не менялась.
fn undo_copy(entry: &Entry) -> io::Result<bool> {
    if !exists(&entry.to) {
        return Ok(false);
    }
    if !exists(&entry.from) {
        return Err(io::Error::other("оригинал пропал — копия оставлена"));
    }
    if fs::metadata(&entry.to)?.len() != entry.size {
        return Err(io::Error::other("копия изменилась — оставлена"));
    }
    fs::remove_file(&entry.to)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history;
    use crate::scanner::tests::{options, run, touch};

    fn sort(
        root: &Path,
        opts: &crate::scanner::ScanOptions,
        mode: Mode,
        remove_empty: bool,
    ) -> (Report, PathBuf) {
        let plan = run(opts);
        let journal_path = root.join("journal.json");
        let job = SortJob {
            source: plan.root.clone(),
            output: plan.output.clone(),
            mode,
            remove_empty,
            moves: plan.moves,
            journal_path: journal_path.clone(),
        };
        (
            run_sort(job, &AtomicBool::new(false), &mut |_, _, _| {}),
            journal_path,
        )
    }

    fn undo(journal_path: &Path) -> Report {
        let journal = history::Journal::load(journal_path).unwrap();
        run_undo(
            journal_path,
            journal,
            &AtomicBool::new(false),
            &mut |_, _, _| {},
        )
    }

    #[test]
    fn move_and_undo_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Downloads");
        touch(&root.join("photo.jpg"), "photo");
        touch(&root.join("song.flac"), "song");
        touch(&root.join("inner/clip.mkv"), "clip");
        touch(&root.join("Изображения/JPG/photo.jpg"), "already there");

        let mut opts = options(&root);
        opts.recursive = true;
        let (report, journal_path) = sort(dir.path(), &opts, Mode::Move, true);
        assert_eq!((report.done, report.failed), (3, 0));
        assert_eq!(
            fs::read_to_string(root.join("Изображения/JPG/photo (1).jpg")).unwrap(),
            "photo"
        );
        assert_eq!(
            fs::read_to_string(root.join("Аудио/FLAC/song.flac")).unwrap(),
            "song"
        );
        assert_eq!(
            fs::read_to_string(root.join("Видео/MKV/clip.mkv")).unwrap(),
            "clip"
        );
        assert!(!root.join("inner").exists(), "опустевшая папка удалена");

        let report = undo(&journal_path);
        assert_eq!((report.done, report.failed), (3, 0));
        assert_eq!(fs::read_to_string(root.join("photo.jpg")).unwrap(), "photo");
        assert_eq!(fs::read_to_string(root.join("song.flac")).unwrap(), "song");
        assert_eq!(
            fs::read_to_string(root.join("inner/clip.mkv")).unwrap(),
            "clip"
        );
        assert!(!root.join("Аудио").exists(), "созданные папки убраны");
        assert!(!root.join("Видео").exists());
        assert_eq!(
            fs::read_to_string(root.join("Изображения/JPG/photo.jpg")).unwrap(),
            "already there"
        );
        assert!(history::Journal::load(&journal_path).unwrap().undone);
    }

    #[test]
    fn copy_and_undo_keeps_originals() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("src");
        let out = dir.path().join("out");
        touch(&root.join("a.txt"), "text");
        touch(&root.join("b.zip"), "zip");

        let mut opts = options(&root);
        opts.output = out.clone();
        let (report, journal_path) = sort(dir.path(), &opts, Mode::Copy, false);
        assert_eq!(report.done, 2);
        assert!(root.join("a.txt").exists());
        assert_eq!(
            fs::read_to_string(out.join("Документы/TXT/a.txt")).unwrap(),
            "text"
        );

        // Изменённую копию отмена не удаляет
        fs::write(out.join("Архивы/ZIP/b.zip"), "changed!").unwrap();
        let report = undo(&journal_path);
        assert_eq!((report.done, report.failed), (1, 1));
        assert!(!out.join("Документы").exists());
        assert!(out.join("Архивы/ZIP/b.zip").exists());
        assert!(root.join("a.txt").exists() && root.join("b.zip").exists());
    }

    #[test]
    fn repeated_copy_skips_existing_copies() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("src");
        touch(&root.join("a.txt"), "text");
        touch(&root.join("b.txt"), "other");
        let mut opts = options(&root);
        opts.output = dir.path().join("out");
        opts.copy = true;
        sort(dir.path(), &opts, Mode::Copy, false);

        let plan = run(&opts);
        assert!(plan.moves.is_empty());
        assert_eq!(plan.in_place, 2);

        // Изменённый оригинал копируется снова, под новым именем
        fs::write(root.join("b.txt"), "changed text").unwrap();
        let plan = run(&opts);
        assert_eq!(plan.moves.len(), 1);
        assert!(plan.moves[0].dst.ends_with("b (1).txt"));
    }

    #[test]
    fn undo_does_not_overwrite_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("d");
        touch(&root.join("note.txt"), "old");
        let (_, journal_path) = sort(dir.path(), &options(&root), Mode::Move, false);
        touch(&root.join("note.txt"), "new");

        let report = undo(&journal_path);
        assert_eq!(report.failed, 0);
        assert_eq!(fs::read_to_string(root.join("note.txt")).unwrap(), "new");
        assert_eq!(
            fs::read_to_string(root.join("note (1).txt")).unwrap(),
            "old"
        );
    }

    #[test]
    fn missing_source_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("d");
        touch(&root.join("a.txt"), "");
        touch(&root.join("b.txt"), "");
        let plan = run(&options(&root));
        fs::remove_file(root.join("a.txt")).unwrap();
        let job = SortJob {
            source: root.clone(),
            output: root.clone(),
            mode: Mode::Move,
            remove_empty: false,
            moves: plan.moves,
            journal_path: dir.path().join("j.json"),
        };
        let report = run_sort(job, &AtomicBool::new(false), &mut |_, _, _| {});
        assert_eq!((report.done, report.failed), (1, 1));
    }
}
