//! Журнал операций: по нему работает отмена.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sorter::Mode;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub from: PathBuf,
    pub to: PathBuf,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Journal {
    /// Время операции для показа, например «26.09.2026 14:03:12».
    pub created: String,
    pub source: PathBuf,
    pub output: PathBuf,
    pub mode: Mode,
    #[serde(default)]
    pub undone: bool,
    pub entries: Vec<Entry>,
    /// Папки, которых не было до сортировки (при отмене удаляются, если пустые).
    #[serde(default)]
    pub created_dirs: Vec<PathBuf>,
    /// Опустевшие папки, удалённые после сортировки (при отмене создаются снова).
    #[serde(default)]
    pub removed_dirs: Vec<PathBuf>,
}

impl Journal {
    pub fn new(source: &Path, output: &Path, mode: Mode) -> Self {
        Self {
            created: chrono::Local::now().format("%d.%m.%Y %H:%M:%S").to_string(),
            source: source.to_path_buf(),
            output: output.to_path_buf(),
            mode,
            undone: false,
            entries: Vec::new(),
            created_dirs: Vec::new(),
            removed_dirs: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }

    /// Запись через временный файл, чтобы сбой не оставил полжурнала.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        fs::rename(&tmp, path)
    }
}

/// Имя файла для нового журнала: history/2026-09-26_14-03-12.json.
pub fn new_journal_path(dir: &Path) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let mut path = dir.join(format!("{stamp}.json"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{stamp}_{n}.json"));
        n += 1;
    }
    path
}

fn journal_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    // Имя — это время, поэтому сортировка по имени = по времени
    files.sort();
    files
}

/// Последняя операция, которую ещё можно отменить.
pub fn last_active(dir: &Path) -> Option<(PathBuf, Journal)> {
    journal_files(dir).into_iter().rev().find_map(|path| {
        let journal = Journal::load(&path).ok()?;
        (!journal.undone && !journal.entries.is_empty()).then_some((path, journal))
    })
}

/// Оставляет только `keep` последних журналов.
pub fn prune(dir: &Path, keep: usize) {
    let files = journal_files(dir);
    let extra = files.len().saturating_sub(keep);
    for path in &files[..extra] {
        let _ = fs::remove_file(path);
    }
}
