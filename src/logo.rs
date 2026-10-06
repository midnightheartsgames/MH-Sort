//! Значок MH Sort, нарисованный кодом: ночная плитка, золотая папка
//! и три разноцветные карточки, выстроенные по росту.
//! Из него собираются иконка окна и иконка exe (см. build.rs).

/// Эскиз задан в квадрате 256×256.
const ART: f32 = 256.0;

struct Layer {
    /// Левый, верхний, правый, нижний край.
    rect: [f32; 4],
    radius: f32,
    /// Цвет сверху и снизу (для плитки — лёгкий градиент).
    top: [u8; 3],
    bottom: [u8; 3],
}

const fn solid(rect: [f32; 4], radius: f32, color: [u8; 3]) -> Layer {
    Layer {
        rect,
        radius,
        top: color,
        bottom: color,
    }
}

const LAYERS: [Layer; 7] = [
    // Плитка «полночь»
    Layer {
        rect: [8.0, 8.0, 248.0, 248.0],
        radius: 58.0,
        top: [0x22, 0x31, 0x58],
        bottom: [0x10, 0x18, 0x2E],
    },
    // Задняя стенка папки с язычком
    solid([40.0, 92.0, 216.0, 206.0], 18.0, [0xC9, 0x98, 0x3C]),
    solid([40.0, 78.0, 118.0, 110.0], 14.0, [0xC9, 0x98, 0x3C]),
    // Карточки-файлы: бирюзовая, фиолетовая, розовая
    solid([66.0, 76.0, 106.0, 190.0], 10.0, [0x45, 0xD0, 0xC0]),
    solid([114.0, 58.0, 154.0, 190.0], 10.0, [0xA6, 0x8C, 0xFF]),
    solid([162.0, 40.0, 202.0, 190.0], 10.0, [0xFF, 0x7A, 0x9A]),
    // Лицевая сторона папки — лунное золото
    solid([32.0, 124.0, 224.0, 214.0], 22.0, [0xF3, 0xC9, 0x69]),
];

/// Расстояние до скруглённого прямоугольника (отрицательное — внутри).
fn distance(layer: &Layer, x: f32, y: f32) -> f32 {
    let [left, top, right, bottom] = layer.rect;
    let half_w = (right - left) / 2.0 - layer.radius;
    let half_h = (bottom - top) / 2.0 - layer.radius;
    let qx = (x - (left + right) / 2.0).abs() - half_w;
    let qy = (y - (top + bottom) / 2.0).abs() - half_h;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - layer.radius
}

/// Пиксели значка size×size в RGBA без предумножения альфы.
pub fn rgba(size: u32) -> Vec<u8> {
    let pixel = ART / size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let x = (px as f32 + 0.5) * pixel;
            let y = (py as f32 + 0.5) * pixel;
            // Накопление в предумноженном виде
            let mut acc = [0.0f32; 4];
            for layer in &LAYERS {
                let coverage = (0.5 - distance(layer, x, y) / pixel).clamp(0.0, 1.0);
                if coverage <= 0.0 {
                    continue;
                }
                let t = ((y - layer.rect[1]) / (layer.rect[3] - layer.rect[1])).clamp(0.0, 1.0);
                for (c, slot) in acc.iter_mut().take(3).enumerate() {
                    let value =
                        layer.top[c] as f32 + (layer.bottom[c] as f32 - layer.top[c] as f32) * t;
                    *slot = value / 255.0 * coverage + *slot * (1.0 - coverage);
                }
                acc[3] = coverage + acc[3] * (1.0 - coverage);
            }
            let alpha = acc[3];
            for channel in &acc[..3] {
                let straight = if alpha > 0.0 { channel / alpha } else { 0.0 };
                out.push((straight * 255.0).round() as u8);
            }
            out.push((alpha * 255.0).round() as u8);
        }
    }
    out
}
