//! Обход папки и построение плана: какой файл куда поедет.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use walkdir::{DirEntry, WalkDir};

use crate::classify::Classifier;
use crate::util::{exists, path_key, unique_path};

#[derive(Clone, Debug, PartialEq)]
pub struct ScanOptions {
    pub root: PathBuf,
    /// Куда складывать категории; обычно совпадает с `root`.
    pub output: PathBuf,
    pub recursive: bool,
    pub type_folders: bool,
    pub skip_sorted: bool,
    pub skip_hidden: bool,
    pub detect_content: bool,
    /// Режим копирования: файлы, чья копия уже лежит на месте, пропускаются.
    pub copy: bool,
    /// Имена папок, в которые не заходим.
    pub excluded: Vec<String>,
    /// Файлы и папки самой программы — их не трогаем никогда.
    pub protected: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct PlannedMove {
    pub src: PathBuf,
    /// Ожидаемый путь назначения (с учётом совпадающих имён).
    pub dst: PathBuf,
    pub category: String,
    pub ext: String,
    pub size: u64,
    pub by_content: bool,
    /// Галочка в предпросмотре.
    pub enabled: bool,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub root: PathBuf,
    pub output: PathBuf,
    pub moves: Vec<PlannedMove>,
    /// Файлы, которые уже лежат в своей папке.
    pub in_place: usize,
    /// Скрытые, системные, недокачанные и прочие пропущенные файлы.
    pub ignored: usize,
    /// Папки, которые не удалось прочитать.
    pub errors: Vec<String>,
}

/// Строит план. Возвращает `None`, если сканирование отменили.
pub fn scan(
    opts: &ScanOptions,
    classifier: &Classifier,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize),
) -> Option<Plan> {
    let same_output = path_key(&opts.root) == path_key(&opts.output);
    let output_key = path_key(&opts.output);
    let protected: HashSet<String> = opts.protected.iter().map(|p| path_key(p)).collect();
    let excluded: HashSet<String> = opts
        .excluded
        .iter()
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .collect();

    let walker = WalkDir::new(&opts.root)
        .min_depth(1)
        .max_depth(if opts.recursive { usize::MAX } else { 1 })
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            if !entry.file_type().is_dir() {
                return true;
            }
            let key = path_key(entry.path());
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let sorted_folder =
                same_output && entry.depth() == 1 && classifier.is_category_folder(&name);
            !(protected.contains(&key)
                || (!same_output && key == output_key)
                || excluded.contains(&name)
                || (opts.skip_hidden && is_hidden(entry))
                || (opts.skip_sorted && sorted_folder))
        });

    let mut plan = Plan {
        root: opts.root.clone(),
        output: opts.output.clone(),
        ..Plan::default()
    };
    let mut reserved = HashSet::new();
    let mut seen = 0;

    for entry in walker {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                plan.errors.push(e.to_string());
                continue;
            }
        };
        // Папки и ссылки (в т.ч. junction) не трогаем
        if !entry.file_type().is_file() {
            continue;
        }
        seen += 1;
        if seen % 256 == 0 {
            progress(seen);
        }

        let path = entry.path();
        let Some(name) = entry.file_name().to_str() else {
            plan.ignored += 1;
            plan.errors.push(format!(
                "Имя файла не в Юникоде, пропущен: {}",
                path.display()
            ));
            continue;
        };
        if protected.contains(&path_key(path))
            || classifier.is_ignored(name)
            || (opts.skip_hidden && is_hidden(&entry))
        {
            plan.ignored += 1;
            continue;
        }

        let class = classifier.classify(path, opts.detect_content);
        let mut dir = opts.output.join(&class.category);
        if opts.type_folders {
            dir.push(&class.type_folder);
        }
        if path
            .parent()
            .is_some_and(|parent| path_key(parent) == path_key(&dir))
            || (opts.copy && is_same_copy(&entry, &dir.join(name)))
        {
            plan.in_place += 1;
            continue;
        }

        let dst = unique_path(&dir, name, &class.ext, |p| {
            reserved.contains(&path_key(p)) || exists(p)
        });
        reserved.insert(path_key(&dst));
        plan.moves.push(PlannedMove {
            src: path.to_path_buf(),
            dst,
            category: class.category,
            ext: class.ext,
            size: entry.metadata().map(|m| m.len()).unwrap_or(0),
            by_content: class.by_content,
            enabled: true,
        });
    }
    progress(seen);
    Some(plan)
}

/// Копия уже лежит в папке назначения: тот же размер и время изменения.
fn is_same_copy(entry: &DirEntry, target: &Path) -> bool {
    let (Ok(src), Ok(dst)) = (entry.metadata(), fs::metadata(target)) else {
        return false;
    };
    let same_time = match (src.modified(), dst.modified()) {
        (Ok(a), Ok(b)) => a
            .duration_since(b)
            .or_else(|_| b.duration_since(a))
            .is_ok_and(|d| d.as_secs() < 2),
        _ => false,
    };
    dst.is_file() && src.len() == dst.len() && same_time
}

fn is_hidden(entry: &DirEntry) -> bool {
    if entry.file_name().to_string_lossy().starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if let Ok(meta) = entry.metadata() {
            return meta.file_attributes() & (HIDDEN | SYSTEM) != 0;
        }
    }
    false
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::Config;
    use std::fs;
    use std::path::Path;

    pub fn touch(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    pub fn options(root: &Path) -> ScanOptions {
        ScanOptions {
            root: root.to_path_buf(),
            output: root.to_path_buf(),
            recursive: false,
            type_folders: true,
            skip_sorted: true,
            skip_hidden: true,
            detect_content: false,
            copy: false,
            excluded: Vec::new(),
            protected: Vec::new(),
        }
    }

    pub fn run(opts: &ScanOptions) -> Plan {
        let classifier = Classifier::new(&Config::default());
        scan(opts, &classifier, &AtomicBool::new(false), &mut |_| {}).unwrap()
    }

    fn rel_dst(plan: &Plan) -> Vec<String> {
        let mut list: Vec<String> = plan
            .moves
            .iter()
            .map(|m| {
                m.dst
                    .strip_prefix(&plan.output)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        list.sort();
        list
    }

    #[test]
    fn root_only_plan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("model.blend"), "");
        touch(&root.join("photo.JPG"), "");
        touch(&root.join("movie.mp4"), "");
        touch(&root.join("setup.exe"), "");
        touch(&root.join("video.mp4.crdownload"), "");
        touch(&root.join(".hidden.txt"), "");
        touch(&root.join("sub/deep.txt"), "");

        let plan = run(&options(root));
        assert_eq!(
            rel_dst(&plan),
            [
                "3D/BLEND/model.blend",
                "Видео/MP4/movie.mp4",
                "Изображения/JPG/photo.JPG",
                "Программы/EXE/setup.exe"
            ]
        );
        assert_eq!(plan.ignored, 2);
    }

    #[test]
    fn recursive_skips_sorted_and_excluded() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("Видео/MP4/old.mp4"), "");
        touch(&root.join("Видео/loose.mp4"), "");
        touch(&root.join("sub/notes.txt"), "");
        touch(&root.join("node_modules/lib.js"), "");

        let mut opts = options(root);
        opts.recursive = true;
        opts.excluded = vec!["NODE_MODULES".into()];
        assert_eq!(rel_dst(&run(&opts)), ["Документы/TXT/notes.txt"]);

        // Без «не трогать отсортированное» разбираются и папки категорий
        opts.skip_sorted = false;
        let plan = run(&opts);
        assert_eq!(
            rel_dst(&plan),
            ["Видео/MP4/loose.mp4", "Документы/TXT/notes.txt"]
        );
        assert_eq!(plan.in_place, 1);
    }

    #[test]
    fn duplicate_names_get_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("Изображения/JPG/image.jpg"), "old");
        touch(&root.join("image.jpg"), "a");
        touch(&root.join("a/image.jpg"), "b");
        touch(&root.join("b/IMAGE.jpg"), "c");

        let mut opts = options(root);
        opts.recursive = true;
        let names = rel_dst(&run(&opts));
        #[cfg(windows)]
        assert_eq!(
            names,
            [
                "Изображения/JPG/IMAGE (2).jpg",
                "Изображения/JPG/image (1).jpg",
                "Изображения/JPG/image (3).jpg"
            ]
        );
        #[cfg(not(windows))]
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn protected_and_output_folders_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("categories.json"), "{}");
        touch(&root.join("Sorted/Документы/TXT/a.txt"), "");
        touch(&root.join("b.txt"), "");

        let mut opts = options(root);
        opts.recursive = true;
        opts.output = root.join("Sorted");
        opts.protected = vec![root.join("categories.json")];
        let plan = run(&opts);
        assert_eq!(rel_dst(&plan), ["Документы/TXT/b.txt"]);
        assert_eq!(plan.ignored, 1);
    }

    #[test]
    fn flat_mode_without_type_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("a.png"), "");
        let mut opts = options(root);
        opts.type_folders = false;
        assert_eq!(rel_dst(&run(&opts)), ["Изображения/a.png"]);
    }
}
