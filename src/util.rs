//! Мелкие помощники: сравнение путей, имена файлов, форматирование.

use std::path::{MAIN_SEPARATOR, Path, PathBuf};

/// Ключ для сравнения путей. На Windows регистр и вид разделителя не важны.
pub fn path_key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    #[cfg(windows)]
    let raw = raw.replace('/', "\\").to_lowercase();
    #[cfg(not(windows))]
    let raw = raw.into_owned();
    let trimmed = raw.trim_end_matches(MAIN_SEPARATOR);
    if trimmed.is_empty() {
        raw
    } else {
        trimmed.to_owned()
    }
}

/// `child` совпадает с `parent` или лежит внутри него.
pub fn is_within(child: &Path, parent: &Path) -> bool {
    let child = path_key(child);
    let parent = path_key(parent);
    if child == parent {
        return true;
    }
    if parent.ends_with(MAIN_SEPARATOR) {
        return child.starts_with(&parent);
    }
    child.starts_with(&parent) && child[parent.len()..].starts_with(MAIN_SEPARATOR)
}

/// Существует ли что-нибудь по этому пути (включая битые ссылки).
pub fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

/// Путь относительно `base` для показа пользователю.
pub fn rel(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Основное расширение файла (после последней точки), в нижнем регистре.
/// Пустая строка, если расширения нет или «хвост» на него не похож
/// (например, «Mr. Smith» или «файл.»).
pub fn primary_ext(file_name: &str) -> String {
    let body = file_name.trim_start_matches('.');
    match body.rfind('.') {
        Some(i) if looks_like_ext(&body[i + 1..]) => body[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

pub fn looks_like_ext(ext: &str) -> bool {
    !ext.is_empty()
        && ext.chars().count() <= 16
        && !ext.contains('.')
        && !ext.chars().any(char::is_whitespace)
        && ext.chars().any(char::is_alphanumeric)
}

/// Делит имя на основу и расширение с точкой: ("archive", ".tar.gz").
/// `ext` — уже известное расширение без точки; пустое значит «расширения нет».
pub fn split_name<'a>(name: &'a str, ext: &str) -> (&'a str, &'a str) {
    if ext.is_empty() {
        return (name, "");
    }
    if name.len() > ext.len() + 1 {
        let cut = name.len() - ext.len() - 1;
        if name.is_char_boundary(cut)
            && name[cut..].starts_with('.')
            && name[cut + 1..].to_lowercase() == ext
        {
            return (&name[..cut], &name[cut..]);
        }
    }
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Первое свободное имя в папке: «image.jpg», «image (1).jpg», «image (2).jpg»…
pub fn unique_path(dir: &Path, name: &str, ext: &str, is_taken: impl Fn(&Path) -> bool) -> PathBuf {
    let first = dir.join(name);
    if !is_taken(&first) {
        return first;
    }
    let (stem, suffix) = split_name(name, ext);
    (1u32..)
        .map(|n| dir.join(format!("{stem} ({n}){suffix}")))
        .find(|p| !is_taken(p))
        .expect("свободное имя всегда находится")
}

/// Делает строку допустимым именем папки Windows.
pub fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Windows не разрешает пробел или точку в конце имени
    let mut result = cleaned.trim().trim_end_matches(['.', ' ']).to_owned();
    if result.is_empty() {
        result.push('_');
    }
    // Имена устройств (CON, NUL, COM1…) запрещены даже с расширением
    let stem_len = result.find('.').unwrap_or(result.len());
    let stem = result[..stem_len].to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        result.insert(stem_len, '_');
    }
    result
}

/// Почему эту папку сортировать нельзя (или `None`, если можно).
pub fn danger_reason(root: &Path, recursive: bool) -> Option<&'static str> {
    for var in [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
    ] {
        if let Some(dir) = std::env::var_os(var)
            && is_within(root, Path::new(&dir))
        {
            return Some("Это системная папка — сортировать её нельзя.");
        }
    }
    if recursive {
        if root.parent().is_none() {
            return Some("Корень диска нельзя сортировать вместе с подпапками.");
        }
        if let Some(home) = dirs::home_dir() {
            let key = path_key(root);
            if key == path_key(&home) || home.parent().is_some_and(|p| key == path_key(p)) {
                return Some("Папку пользователя нельзя сортировать вместе с подпапками.");
            }
        }
    }
    None
}

/// «3,2 ГБ», «512 Б».
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
    if bytes < 1024 {
        return format!("{bytes} Б");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let number = if value < 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    };
    format!("{} {}", number.replace('.', ","), UNITS[unit])
}

/// Русское склонение: 1 файл, 2 файла, 5 файлов.
pub fn plural<'a>(n: usize, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    match (n % 10, n % 100) {
        (_, 11..=14) => many,
        (1, _) => one,
        (2..=4, _) => few,
        _ => many,
    }
}

/// Число с разбивкой на разряды неразрывным пробелом: «1 284».
pub fn group(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{a0}');
        }
        out.push(digit);
    }
    out
}

pub fn files(n: usize) -> String {
    format!("{} {}", group(n), plural(n, "файл", "файла", "файлов"))
}

/// Показать файл в проводнике.
pub fn reveal(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn();
    }
    #[cfg(not(windows))]
    if let Some(parent) = path.parent() {
        let _ = open::that_detached(parent);
    }
}

/// Открыть файл или папку программой по умолчанию.
pub fn open_path(path: &Path) -> Result<(), String> {
    open::that_detached(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plural_forms() {
        assert_eq!(files(1), "1 файл");
        assert_eq!(files(3), "3 файла");
        assert_eq!(files(11), "11 файлов");
        assert_eq!(files(22), "22 файла");
        assert_eq!(files(105), "105 файлов");
        assert_eq!(files(1284), "1\u{a0}284 файла");
        assert_eq!(group(1_000_000), "1\u{a0}000\u{a0}000");
    }

    #[test]
    fn sizes() {
        assert_eq!(format_size(512), "512 Б");
        assert_eq!(format_size(1536), "1,5 КБ");
        assert_eq!(
            format_size(3 * 1024 * 1024 * 1024 + 300 * 1024 * 1024),
            "3,3 ГБ"
        );
    }

    #[test]
    fn name_splitting() {
        assert_eq!(split_name("photo.jpg", "jpg"), ("photo", ".jpg"));
        assert_eq!(split_name("backup.TAR.GZ", "tar.gz"), ("backup", ".TAR.GZ"));
        assert_eq!(split_name("Mr. Smith", ""), ("Mr. Smith", ""));
        assert_eq!(primary_ext("Фото.JPEG"), "jpeg");
        assert_eq!(primary_ext(".gitignore"), "");
        assert_eq!(primary_ext("Mr. Smith goes"), "");
        assert_eq!(primary_ext("файл."), "");
    }

    #[test]
    fn unique_names() {
        let taken = ["/x/a.jpg", "/x/a (1).jpg"].map(PathBuf::from);
        let path = unique_path(Path::new("/x"), "a.jpg", "jpg", |p| {
            taken.contains(&p.to_path_buf())
        });
        assert_eq!(path, PathBuf::from("/x").join("a (2).jpg"));
    }

    #[test]
    fn sanitizing() {
        assert_eq!(sanitize_name("Игры / ресурсы"), "Игры _ ресурсы");
        assert_eq!(sanitize_name("CON"), "CON_");
        assert_eq!(sanitize_name("com1.txt"), "com1_.txt");
        assert_eq!(sanitize_name("COM0"), "COM0");
        assert_eq!(sanitize_name("Видео. "), "Видео");
        assert_eq!(sanitize_name(""), "_");
    }

    #[cfg(windows)]
    #[test]
    fn dangerous_folders_are_blocked() {
        let home = dirs::home_dir().unwrap();
        assert!(danger_reason(Path::new(r"C:\"), true).is_some());
        assert!(danger_reason(Path::new(r"C:\"), false).is_none());
        assert!(danger_reason(Path::new(r"C:\Windows\Temp"), false).is_some());
        assert!(danger_reason(&home, true).is_some());
        assert!(danger_reason(&home, false).is_none());
        assert!(danger_reason(&home.join("Downloads"), true).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn paths_compare_case_insensitive() {
        assert!(is_within(
            Path::new(r"D:\Downloads\Видео\a.mp4"),
            Path::new(r"d:/downloads/")
        ));
        assert!(!is_within(
            Path::new(r"D:\Downloads2"),
            Path::new(r"D:\Downloads")
        ));
        assert!(is_within(Path::new(r"D:\x"), Path::new(r"D:\")));
        assert_eq!(
            path_key(Path::new(r"D:\Видео\")),
            path_key(Path::new("d:/видео"))
        );
    }
}
