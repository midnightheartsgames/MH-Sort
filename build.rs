//! На Windows (MSVC) встраивает в exe иконку из src/logo.rs.
//! .res пишется напрямую — rc.exe не нужен, link.exe принимает .res сам.

use std::path::PathBuf;
use std::{env, fs};

#[path = "src/logo.rs"]
mod logo;

const SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

fn main() {
    println!("cargo:rerun-if-changed=src/logo.rs");
    let windows = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !(windows && msvc) {
        return;
    }
    let res = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("icon.res");
    fs::write(&res, icon_resources()).expect("не удалось записать icon.res");
    println!("cargo:rustc-link-arg-bins={}", res.display());
}

/// Ресурсы RT_ICON для каждого размера и одна группа RT_GROUP_ICON с id 1.
fn icon_resources() -> Vec<u8> {
    let mut res = Vec::new();
    // Пустая запись в начале — признак 32-битного .res
    push_resource(&mut res, 0, 0, 0, &[]);
    let mut group = Vec::new();
    group.extend(0u16.to_le_bytes());
    group.extend(1u16.to_le_bytes());
    group.extend((SIZES.len() as u16).to_le_bytes());
    for (index, &size) in SIZES.iter().enumerate() {
        let id = index as u16 + 1;
        let image = bitmap(size);
        push_resource(&mut res, RT_ICON, id, 0x1010, &image);
        // 256 записывается как 0
        let side = if size >= 256 { 0 } else { size as u8 };
        group.extend([side, side, 0, 0]);
        group.extend(1u16.to_le_bytes());
        group.extend(32u16.to_le_bytes());
        group.extend((image.len() as u32).to_le_bytes());
        group.extend(id.to_le_bytes());
    }
    push_resource(&mut res, RT_GROUP_ICON, 1, 0x1030, &group);
    res
}

fn push_resource(res: &mut Vec<u8>, kind: u16, id: u16, flags: u16, data: &[u8]) {
    res.extend((data.len() as u32).to_le_bytes());
    res.extend(32u32.to_le_bytes()); // размер заголовка
    res.extend([0xFF, 0xFF]);
    res.extend(kind.to_le_bytes());
    res.extend([0xFF, 0xFF]);
    res.extend(id.to_le_bytes());
    res.extend(0u32.to_le_bytes()); // DataVersion
    res.extend(flags.to_le_bytes());
    res.extend(0u16.to_le_bytes()); // язык: нейтральный
    res.extend(0u32.to_le_bytes()); // Version
    res.extend(0u32.to_le_bytes()); // Characteristics
    res.extend(data);
    while !res.len().is_multiple_of(4) {
        res.push(0);
    }
}

/// Картинка иконки в формате DIB: BGRA снизу вверх и маска прозрачности.
fn bitmap(size: u32) -> Vec<u8> {
    let rgba = logo::rgba(size);
    let side = size as usize;
    let mask_stride = side.div_ceil(32) * 4;
    let mut data = Vec::new();
    data.extend(40u32.to_le_bytes());
    data.extend((size as i32).to_le_bytes());
    data.extend((size as i32 * 2).to_le_bytes()); // высота удвоена: цвет + маска
    data.extend(1u16.to_le_bytes());
    data.extend(32u16.to_le_bytes());
    data.extend(0u32.to_le_bytes()); // BI_RGB
    data.extend(((side * side * 4 + mask_stride * side) as u32).to_le_bytes());
    data.extend([0u8; 16]);
    for y in (0..side).rev() {
        for x in 0..side {
            let i = (y * side + x) * 4;
            data.extend([rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    for y in (0..side).rev() {
        let mut row = vec![0u8; mask_stride];
        for x in 0..side {
            if rgba[(y * side + x) * 4 + 3] == 0 {
                row[x / 8] |= 0x80 >> (x % 8);
            }
        }
        data.extend(row);
    }
    data
}
