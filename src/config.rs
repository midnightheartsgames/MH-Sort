//! Файлы программы: categories.json (категории) и settings.json (галочки в окне).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::sorter::Mode;

pub const CONFIG_FILE: &str = "categories.json";

/// Где лежат файлы программы.
pub struct Paths {
    pub config: PathBuf,
    pub settings: PathBuf,
    pub history: PathBuf,
}

impl Paths {
    /// Портативный режим, если рядом с exe лежит categories.json, иначе %APPDATA%\MH Sort.
    pub fn detect() -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        let dir = match exe_dir {
            Some(dir) if dir.join(CONFIG_FILE).is_file() => dir,
            other => dirs::config_dir()
                .map(|d| d.join("MH Sort"))
                .or(other)
                .unwrap_or_else(|| PathBuf::from(".")),
        };
        Self::in_dir(&dir)
    }

    pub fn in_dir(dir: &Path) -> Self {
        Self {
            config: dir.join(CONFIG_FILE),
            settings: dir.join("settings.json"),
            history: dir.join("history"),
        }
    }
}

/// Содержимое categories.json.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Категория для файлов, которые ни к чему не подошли.
    pub unknown_category: String,
    /// Папка типа для файлов без расширения.
    pub no_extension_folder: String,
    /// Такие файлы не трогаются: недокачанные и временные.
    pub ignore_extensions: Vec<String>,
    /// Категория → расширения. Если расширение указано дважды, побеждает первая категория.
    pub categories: IndexMap<String, Vec<String>>,
}

const DEFAULT_CATEGORIES: &[(&str, &[&str])] = &[
    (
        "Изображения",
        &[
            "jpg", "jpeg", "jfif", "png", "webp", "gif", "bmp", "tif", "tiff", "svg", "ico",
            "heic", "heif", "avif", "raw", "cr2", "cr3", "nef", "arw", "dng", "tga", "exr", "hdr",
            "dds",
        ],
    ),
    (
        "Видео",
        &[
            "mp4", "mkv", "avi", "mov", "webm", "mpg", "mpeg", "m4v", "wmv", "flv", "3gp", "m2ts",
            "mts", "vob", "srt", "ass", "vtt",
        ],
    ),
    (
        "Аудио",
        &[
            "mp3", "wav", "flac", "ogg", "m4a", "aac", "opus", "wma", "aiff", "ape", "mid", "midi",
        ],
    ),
    (
        "Документы",
        &[
            "txt", "pdf", "doc", "docx", "odt", "rtf", "md", "log", "xps", "mht", "mhtml",
        ],
    ),
    (
        "Книги",
        &["epub", "fb2", "mobi", "azw3", "djvu", "cbz", "cbr"],
    ),
    ("Таблицы", &["xls", "xlsx", "ods", "csv", "tsv"]),
    ("Презентации", &["ppt", "pptx", "odp"]),
    (
        "Архивы",
        &[
            "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "tar.gz", "tar.xz",
            "tar.bz2", "tar.zst",
        ],
    ),
    (
        "Образы дисков",
        &["iso", "img", "vhd", "vhdx", "vmdk", "dmg"],
    ),
    (
        "3D",
        &[
            "blend", "blend1", "fbx", "obj", "mtl", "gltf", "glb", "dae", "stl", "3ds", "max",
            "ma", "mb", "usd", "usda", "usdc", "usdz", "abc", "ply", "3mf", "c4d", "ztl", "zpr",
            "vox",
        ],
    ),
    (
        "Проекты",
        &[
            "psd", "psb", "kra", "clip", "sai", "sai2", "xcf", "ai", "afphoto", "afdesign", "aep",
            "prproj", "drp", "spp", "sbs", "sbsar", "indd", "fla",
        ],
    ),
    (
        "Код",
        &[
            "py", "js", "ts", "jsx", "tsx", "c", "cpp", "h", "hpp", "rs", "java", "kt", "cs", "go",
            "rb", "php", "lua", "swift", "sh", "bat", "cmd", "ps1", "html", "css", "json", "xml",
            "yaml", "yml", "toml", "ini", "sql", "gd", "shader", "hlsl", "glsl",
        ],
    ),
    (
        "Игровые ресурсы",
        &[
            "pak",
            "wad",
            "vpk",
            "unitypackage",
            "uasset",
            "umap",
            "bsa",
            "ba2",
            "esp",
            "esm",
            "rpf",
        ],
    ),
    (
        "Программы",
        &[
            "exe",
            "msi",
            "appx",
            "msix",
            "appxbundle",
            "msixbundle",
            "apk",
            "xapk",
            "jar",
            "deb",
            "rpm",
            "appimage",
        ],
    ),
    (
        "AI модели",
        &[
            "safetensors",
            "gguf",
            "ggml",
            "ckpt",
            "pt",
            "pth",
            "onnx",
            "tflite",
            "keras",
        ],
    ),
    ("Торренты", &["torrent"]),
    ("Шрифты", &["ttf", "otf", "woff", "woff2", "ttc", "fon"]),
    ("Ярлыки", &["lnk", "url"]),
];

impl Default for Config {
    fn default() -> Self {
        let words = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        Self {
            unknown_category: "Прочее".into(),
            no_extension_folder: "Без расширения".into(),
            ignore_extensions: words(&[
                "crdownload",
                "part",
                "partial",
                "download",
                "opdownload",
                "downloading",
                "tmp",
                "!ut",
                "!qb",
                "aria2",
            ]),
            categories: DEFAULT_CATEGORIES
                .iter()
                .map(|(name, exts)| (name.to_string(), words(exts)))
                .collect(),
        }
    }
}

impl Config {
    /// Читает конфиг. Если файла нет — создаёт стандартный.
    /// Если файл испорчен — возвращает стандартный конфиг и текст ошибки, сам файл не трогает.
    pub fn load_or_create(path: &Path) -> (Self, Option<String>) {
        match fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
                Ok(config) => (config, None),
                Err(e) => (
                    Self::default(),
                    Some(format!(
                        "Ошибка в {CONFIG_FILE}: {e}. Пока используются стандартные категории."
                    )),
                ),
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let config = Self::default();
                let error = config
                    .save(path)
                    .err()
                    .map(|e| format!("Не удалось создать {}: {e}", path.display()));
                (config, error)
            }
            Err(e) => (
                Self::default(),
                Some(format!("Не удалось прочитать {}: {e}", path.display())),
            ),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, self.to_json())
    }

    /// JSON, удобный для ручной правки: каждая категория в одну строку.
    pub fn to_json(&self) -> String {
        let quote = |s: &str| serde_json::to_string(s).unwrap_or_default();
        let list = |items: &[String]| {
            items
                .iter()
                .map(|s| quote(s))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut out = String::from("{\n");
        out += &format!(
            "  \"unknown_category\": {},\n",
            quote(&self.unknown_category)
        );
        out += &format!(
            "  \"no_extension_folder\": {},\n",
            quote(&self.no_extension_folder)
        );
        out += &format!(
            "  \"ignore_extensions\": [{}],\n",
            list(&self.ignore_extensions)
        );
        out += "  \"categories\": {\n";
        let last = self.categories.len().saturating_sub(1);
        for (i, (name, exts)) in self.categories.iter().enumerate() {
            let comma = if i == last { "" } else { "," };
            out += &format!("    {}: [{}]{comma}\n", quote(name), list(exts));
        }
        out += "  }\n}\n";
        out
    }
}

/// Состояние окна между запусками.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub folder: String,
    pub use_output: bool,
    pub output: String,
    pub mode: Mode,
    pub recursive: bool,
    pub type_folders: bool,
    pub skip_sorted: bool,
    pub remove_empty: bool,
    pub skip_hidden: bool,
    pub detect_content: bool,
    pub excluded: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            folder: String::new(),
            use_output: false,
            output: String::new(),
            mode: Mode::Move,
            recursive: false,
            type_folders: true,
            skip_sorted: true,
            remove_empty: false,
            skip_hidden: true,
            detect_content: true,
            excluded: Vec::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrip() {
        let config = Config::default();
        let parsed: Config = serde_json::from_str(&config.to_json()).unwrap();
        assert_eq!(parsed.categories, config.categories);
        assert_eq!(parsed.unknown_category, "Прочее");
        assert_eq!(parsed.categories.keys().next().unwrap(), "Изображения");
    }

    #[test]
    fn broken_config_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "{ broken").unwrap();
        let (config, error) = Config::load_or_create(&path);
        assert!(error.is_some());
        assert!(!config.categories.is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");
    }

    #[test]
    fn missing_config_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join(CONFIG_FILE);
        let (_, error) = Config::load_or_create(&path);
        assert!(error.is_none());
        assert!(path.is_file());
    }
}
