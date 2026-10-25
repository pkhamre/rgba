//! Software scanline renderer. Mid-scanline fetch/contention accuracy is pending.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Pixel {
    color: u16,
    layer: u8,
    priority: u8,
    semi: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Ppu {
    #[serde(deserialize_with = "crate::state::deserialize_bounded_vec")]
    pub(crate) pixels: Vec<u32>,
    reference: [[i32; 2]; 2],
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            pixels: vec![0xffffff; 240 * 160],
            reference: [[0; 2]; 2],
        }
    }
}

fn half(bytes: &[u8], offset: usize) -> u16 {
    // Almost all PPU register/palette/tile reads are already inside the slice.
    // Avoid two integer divisions for each of these hot-path halfword reads;
    // retain the original wrapping behavior for out-of-range addresses.
    if offset < bytes.len().saturating_sub(1) {
        u16::from(bytes[offset]) | (u16::from(bytes[offset + 1]) << 8)
    } else {
        u16::from(bytes[offset % bytes.len()]) | (u16::from(bytes[(offset + 1) % bytes.len()]) << 8)
    }
}

fn signed_reference(io: &[u8], offset: usize) -> i32 {
    let value = u32::from(half(io, offset)) | (u32::from(half(io, offset + 2)) << 16);
    ((value << 4) as i32) >> 4
}

fn inside(value: usize, start: usize, end: usize) -> bool {
    if start <= end {
        (start..end).contains(&value)
    } else {
        value >= start || value < end
    }
}

// These values are local to one render call, not persistent caches. Register
// writes between scanlines therefore need no invalidation or save-state changes.
struct BackgroundLine {
    mode: u16,
    control: u16,
    mosaic_width: usize,
    text: bool,
    affine: bool,
    width: usize,
    scroll_x: usize,
    sy: usize,
    map_base: usize,
    tile_base: usize,
    affine_step: [i32; 2],
    affine_size: i32,
    page: usize,
}

impl BackgroundLine {
    fn new(bg: usize, y: usize, display: u16, io: &[u8]) -> Self {
        let mode = display & 7;
        let control = half(io, 8 + bg * 2);
        let size = (control >> 14) as usize;
        let width = if size & 1 != 0 { 512 } else { 256 };
        let height = if size & 2 != 0 { 512 } else { 256 };
        let mosaic = control & 0x40 != 0;
        let mosaic_width = if mosaic {
            (io[0x4c] as usize & 15) + 1
        } else {
            1
        };
        let y = if mosaic {
            y - y % ((io[0x4c] as usize >> 4) + 1)
        } else {
            y
        };
        let affine_step = if bg >= 2 {
            [
                i32::from(half(io, 0x20 + (bg - 2) * 0x10) as i16),
                i32::from(half(io, 0x24 + (bg - 2) * 0x10) as i16),
            ]
        } else {
            [0; 2]
        };
        Self {
            mode,
            control,
            mosaic_width,
            text: mode == 0 || (mode == 1 && bg < 2),
            affine: (mode == 1 && bg == 2) || (mode == 2 && bg >= 2),
            width,
            scroll_x: (half(io, 0x10 + bg * 4) & 511) as usize,
            sy: (y + (half(io, 0x12 + bg * 4) & 511) as usize) % height,
            map_base: ((control >> 8) & 31) as usize * 0x800,
            tile_base: ((control >> 2) & 3) as usize * 0x4000,
            affine_step,
            affine_size: 128 << (control >> 14),
            page: if mode != 3 && display & 0x10 != 0 {
                0xa000
            } else {
                0
            },
        }
    }
}

// One decoded tile row per text background. The source tile column is enough
// to identify it because all other configuration is fixed for this scanline.
#[derive(Default)]
struct TextTileRow {
    column: Option<usize>,
    colors: [Option<u16>; 8],
}

impl TextTileRow {
    fn sample(
        &mut self,
        sx: usize,
        line: &BackgroundLine,
        palette: &[u8],
        vram: &[u8],
    ) -> Option<u16> {
        let column = sx / 8;
        if self.column != Some(column) {
            let sy = line.sy;
            let block = sx / 256 + (sy / 256) * (line.width / 256);
            let map = line.map_base + block * 0x800 + (((sy % 256) / 8) * 32 + (sx % 256) / 8) * 2;
            let entry = half(vram, map & 0xffff);
            let py = if entry & 0x800 != 0 {
                7 - sy % 8
            } else {
                sy % 8
            };
            let tile = (entry & 0x3ff) as usize;
            let (texels, shift, mask, bank) = if line.control & 0x80 != 0 {
                let row = line.tile_base + tile * 64 + py * 8;
                let bytes = std::array::from_fn(|index| vram[(row + index) & 0xffff]);
                (u64::from_le_bytes(bytes), 8, 255, 0)
            } else {
                let row = line.tile_base + tile * 32 + py * 4;
                let bytes = std::array::from_fn(|index| vram[(row + index) & 0xffff]);
                (
                    u64::from(u32::from_le_bytes(bytes)),
                    4,
                    15,
                    (entry >> 12) as usize * 16,
                )
            };
            for (x, color) in self.colors.iter_mut().enumerate() {
                let px = if entry & 0x400 != 0 { 7 - x } else { x };
                let index = ((texels >> (px * shift)) & mask) as usize;
                *color = if index == 0 {
                    None
                } else {
                    Some(half(palette, (index + bank) * 2))
                };
            }
            self.column = Some(column);
        }
        self.colors[sx & 7]
    }
}

struct WindowLine {
    active: [bool; 2],
    horizontal: [u16; 2],
    masks: [u16; 2],
    outside: u16,
    object: u16,
    enabled: bool,
    object_enabled: bool,
}

impl WindowLine {
    fn new(y: usize, display: u16, io: &[u8]) -> Self {
        let active = std::array::from_fn(|window| {
            let vertical = half(io, 0x44 + window * 2);
            display & (0x2000 << window) != 0
                && inside(y, (vertical >> 8) as usize, (vertical & 255) as usize)
        });
        let masks = half(io, 0x48);
        let outside = half(io, 0x4a);
        Self {
            active,
            horizontal: [half(io, 0x40), half(io, 0x42)],
            masks: [masks & 0x3f, (masks >> 8) & 0x3f],
            outside: outside & 0x3f,
            object: (outside >> 8) & 0x3f,
            enabled: display & 0xe000 != 0,
            object_enabled: display & 0x8000 != 0,
        }
    }

    fn mask(&self, x: usize, object: bool) -> u16 {
        if !self.enabled {
            return 0x3f;
        }
        for window in 0..2 {
            let horizontal = self.horizontal[window];
            if self.active[window]
                && inside(x, (horizontal >> 8) as usize, (horizontal & 255) as usize)
            {
                return self.masks[window];
            }
        }
        if object && self.object_enabled {
            self.object
        } else {
            self.outside
        }
    }
}

impl Ppu {
    pub(crate) fn io_written(&mut self, io: &[u8], offset: usize) {
        for bg in 0..2 {
            for axis in 0..2 {
                let address = 0x28 + bg * 0x10 + axis * 4;
                if (address..address + 4).contains(&offset) {
                    self.reference[bg][axis] = signed_reference(io, address);
                }
            }
        }
    }

    pub(crate) fn render_line(
        &mut self,
        y: usize,
        io: &[u8],
        palette: &[u8],
        vram: &[u8],
        oam: &[u8],
    ) {
        if y == 0 {
            for bg in 0..2 {
                self.reference[bg] = [
                    signed_reference(io, 0x28 + bg * 0x10),
                    signed_reference(io, 0x2c + bg * 0x10),
                ];
            }
        }
        let display = half(io, 0);
        if display & 0x80 != 0 {
            self.pixels[y * 240..(y + 1) * 240].fill(0xffffff);
        } else {
            let backdrop = Pixel {
                color: half(palette, 0),
                layer: 5,
                priority: 4,
                semi: false,
            };
            let mut objects = [None; 240];
            let mut object_window = [false; 240];
            if display & 0x9000 != 0 {
                Self::objects(y, io, palette, vram, oam, &mut objects, &mut object_window);
            }
            let backgrounds: [BackgroundLine; 4] =
                std::array::from_fn(|bg| BackgroundLine::new(bg, y, display, io));
            let mut text_rows: [TextTileRow; 4] = std::array::from_fn(|_| TextTileRow::default());
            let windows = WindowLine::new(y, display, io);
            let blend = half(io, 0x50);
            let effect = (blend >> 6) & 3;
            let alpha = half(io, 0x52);
            let eva = (alpha & 31).min(16);
            let evb = ((alpha >> 8) & 31).min(16);
            let brightness = (half(io, 0x54) & 31).min(16);
            for x in 0..240 {
                let mask = windows.mask(x, object_window[x]);
                let mut top = backdrop;
                let mut below = None;
                for (bg, background) in backgrounds.iter().enumerate() {
                    if display & (0x100 << bg) == 0 || mask & (1 << bg) == 0 {
                        continue;
                    }
                    if let Some(pixel) =
                        self.background(bg, x, background, &mut text_rows[bg], palette, vram)
                    {
                        Self::insert_layer(pixel, &mut top, &mut below);
                    }
                }
                if display & 0x1000 != 0
                    && mask & 0x10 != 0
                    && let Some(pixel) = objects[x]
                {
                    Self::insert_layer(pixel, &mut top, &mut below);
                }
                let below = below.unwrap_or(backdrop);
                let mut color = top.color;
                let first = blend & (1 << top.layer) != 0;
                let second = blend & (1 << (8 + below.layer)) != 0;
                let effects_enabled = mask & 0x20 != 0;
                // Semi-transparent OBJs force alpha blending even when the window
                // disables ordinary color effects. They still need a second target.
                if (top.semi || (effects_enabled && effect == 1 && first)) && second {
                    color = Self::mix(top.color, below.color, eva, evb);
                } else if effects_enabled && first && (effect == 2 || effect == 3) {
                    color = Self::brightness(color, brightness, effect == 2);
                }
                self.pixels[y * 240 + x] = Self::rgb(color);
            }
        }
        for bg in 0..2 {
            self.reference[bg][0] += i32::from(half(io, 0x22 + bg * 0x10) as i16);
            self.reference[bg][1] += i32::from(half(io, 0x26 + bg * 0x10) as i16);
        }
    }

    fn insert_layer(pixel: Pixel, top: &mut Pixel, below: &mut Option<Pixel>) {
        let key = |pixel: Pixel| {
            (
                pixel.priority,
                if pixel.layer == 4 { 0 } else { pixel.layer + 1 },
            )
        };
        // Only the front two layers affect the existing compositor. Strict
        // comparisons preserve insertion order for ties, just like stable sort.
        if key(pixel) < key(*top) {
            *below = Some(*top);
            *top = pixel;
        } else if below.is_none_or(|old| key(pixel) < key(old)) {
            *below = Some(pixel);
        }
    }

    fn background(
        &self,
        bg: usize,
        mut x: usize,
        line: &BackgroundLine,
        text_row: &mut TextTileRow,
        palette: &[u8],
        vram: &[u8],
    ) -> Option<Pixel> {
        let mode = line.mode;
        let control = line.control;
        if line.mosaic_width != 1 {
            x -= x % line.mosaic_width;
        }
        let color = if line.text {
            let sx = (x + line.scroll_x) % line.width;
            text_row.sample(sx, line, palette, vram)?
        } else if line.affine || ((3..=5).contains(&mode) && bg == 2) {
            let affine_index = bg - 2;
            let [pa, pc] = line.affine_step;
            let mut sx = (self.reference[affine_index][0] + pa * x as i32) >> 8;
            let mut sy = (self.reference[affine_index][1] + pc * x as i32) >> 8;
            if line.affine {
                let size = line.affine_size;
                if control & 0x2000 != 0 {
                    sx = sx.rem_euclid(size);
                    sy = sy.rem_euclid(size);
                } else if sx < 0 || sy < 0 || sx >= size || sy >= size {
                    return None;
                }
                let map = line.map_base;
                let tile = vram
                    [(map + (sy as usize / 8) * (size as usize / 8) + sx as usize / 8) & 0xffff]
                    as usize;
                let base = line.tile_base;
                let index =
                    vram[(base + tile * 64 + (sy as usize & 7) * 8 + (sx as usize & 7)) & 0xffff];
                if index == 0 {
                    return None;
                }
                half(palette, index as usize * 2)
            } else {
                let (width, height) = if mode == 5 { (160, 128) } else { (240, 160) };
                if sx < 0 || sy < 0 || sx >= width || sy >= height {
                    return None;
                }
                let page = line.page;
                let offset = sy as usize * width as usize + sx as usize;
                if mode == 4 {
                    half(palette, vram[page + offset] as usize * 2)
                } else {
                    half(vram, page + offset * 2)
                }
            }
        } else {
            return None;
        };
        Some(Pixel {
            color,
            layer: bg as u8,
            priority: (control & 3) as u8,
            semi: false,
        })
    }

    fn objects(
        y: usize,
        io: &[u8],
        palette: &[u8],
        vram: &[u8],
        oam: &[u8],
        pixels: &mut [Option<Pixel>; 240],
        window: &mut [bool; 240],
    ) {
        let sizes = [
            [(8, 8), (16, 16), (32, 32), (64, 64)],
            [(16, 8), (32, 8), (32, 16), (64, 32)],
            [(8, 16), (8, 32), (16, 32), (32, 64)],
        ];
        for object in 0..128 {
            let a = half(oam, object * 8);
            let b = half(oam, object * 8 + 2);
            let c = half(oam, object * 8 + 4);
            let affine = a & 0x100 != 0;
            if !affine && a & 0x200 != 0 {
                continue;
            }
            let shape = (a >> 14) as usize;
            if shape == 3 || (a >> 10) & 3 == 3 {
                continue;
            }
            let (width, height) = sizes[shape][(b >> 14) as usize];
            let double = affine && a & 0x200 != 0;
            let box_width = if double { width * 2 } else { width };
            let box_height = if double { height * 2 } else { height };
            let top = (a & 255) as usize;
            let local_y = y.wrapping_sub(top) & 255;
            if local_y >= box_height {
                continue;
            }
            let left = (b & 511) as i32;
            let left = if left >= 256 { left - 512 } else { left };
            for local_x in 0..box_width {
                let screen_x = left + local_x as i32;
                if !(0..240).contains(&screen_x) {
                    continue;
                }
                let mut ox = local_x as i32;
                let mut oy = local_y as i32;
                if a & 0x1000 != 0 {
                    ox -= ox % (((io[0x4d] & 15) + 1) as i32);
                    oy -= oy % (((io[0x4d] >> 4) + 1) as i32);
                }
                if affine {
                    let matrix = ((b >> 9) & 31) as usize * 32;
                    let pa = i32::from(half(oam, matrix + 6) as i16);
                    let pb = i32::from(half(oam, matrix + 14) as i16);
                    let pc = i32::from(half(oam, matrix + 22) as i16);
                    let pd = i32::from(half(oam, matrix + 30) as i16);
                    let cx = ox - box_width as i32 / 2;
                    let cy = oy - box_height as i32 / 2;
                    ox = ((pa * cx + pb * cy) >> 8) + width as i32 / 2;
                    oy = ((pc * cx + pd * cy) >> 8) + height as i32 / 2;
                } else {
                    if b & 0x1000 != 0 {
                        ox = width as i32 - 1 - ox;
                    }
                    if b & 0x2000 != 0 {
                        oy = height as i32 - 1 - oy;
                    }
                }
                if ox < 0 || oy < 0 || ox >= width as i32 || oy >= height as i32 {
                    continue;
                }
                let color256 = a & 0x2000 != 0;
                let tile_units = if color256 { 2 } else { 1 };
                let stride = if half(io, 0) & 0x40 != 0 {
                    width / 8 * tile_units
                } else {
                    32
                };
                let tile = ((c & 0x3ff) as usize & if color256 { !1 } else { usize::MAX })
                    + oy as usize / 8 * stride
                    + ox as usize / 8 * tile_units;
                let address = 0x10000
                    + ((tile * 32
                        + (oy as usize & 7) * if color256 { 8 } else { 4 }
                        + (ox as usize & 7) / if color256 { 1 } else { 2 })
                        & 0x7fff);
                if half(io, 0) & 7 >= 3 && address < 0x14000 {
                    continue;
                }
                let byte = vram[address];
                let index = if color256 {
                    byte as usize
                } else {
                    ((byte >> ((ox as usize & 1) * 4)) & 15) as usize
                };
                if index == 0 {
                    continue;
                }
                let x = screen_x as usize;
                if (a >> 10) & 3 == 2 {
                    window[x] = true;
                    continue;
                }
                let priority = ((c >> 10) & 3) as u8;
                if pixels[x].is_none_or(|old| priority < old.priority) {
                    let bank = if color256 { 0 } else { (c >> 12) as usize * 16 };
                    pixels[x] = Some(Pixel {
                        color: half(palette, 0x200 + (index + bank) * 2),
                        layer: 4,
                        priority,
                        semi: a & 0xc00 == 0x400,
                    });
                }
            }
        }
    }

    fn rgb(color: u16) -> u32 {
        let red = u32::from(color & 31) * 255 / 31;
        let green = u32::from((color >> 5) & 31) * 255 / 31;
        let blue = u32::from((color >> 10) & 31) * 255 / 31;
        (red << 16) | (green << 8) | blue
    }

    fn mix(a: u16, b: u16, eva: u16, evb: u16) -> u16 {
        let mut result = 0;
        for shift in [0, 5, 10] {
            result |=
                ((((a >> shift) & 31) * eva + ((b >> shift) & 31) * evb) / 16).min(31) << shift;
        }
        result
    }

    fn brightness(color: u16, amount: u16, brighten: bool) -> u16 {
        let mut result = 0;
        for shift in [0, 5, 10] {
            let channel = (color >> shift) & 31;
            let value = if brighten {
                channel + (31 - channel) * amount / 16
            } else {
                channel - channel * amount / 16
            };
            result |= value << shift;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::Ppu;

    #[test]
    fn scanlines_match_original_renderer_fingerprints() {
        // Fingerprints captured before scanline configuration decoding was introduced.
        // Exercise every mode (including unsupported modes), window enable combination,
        // background size/depth/flip, mosaic, affine clipping/wrapping and color effect.
        let mut seed = 0x9852_147bu32;
        let mut random_byte = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        };
        let mut palette = [0; 0x400];
        let mut vram = vec![0; 0x18000];
        let mut oam = [0; 0x400];
        for bytes in [&mut palette[..], &mut vram[..], &mut oam[..]] {
            for byte in bytes {
                *byte = random_byte();
            }
        }
        for mode in 0..8 {
            let mut hash = 0xcbf2_9ce4_8422_2325u64;
            for case in 0..128 {
                let mut io = [0; 0x400];
                for byte in &mut io {
                    *byte = random_byte();
                }
                io[0] = mode | if case & 1 != 0 { 0x10 } else { 0 };
                io[1] = 0x1f | ((case & 7) << 5);
                // Keep affine samples visible as well as testing negative/outside coordinates.
                for bg in 0..2 {
                    let base = 0x20 + bg * 0x10;
                    for (offset, value) in [(0, 256i16), (2, -128), (4, 64), (6, 256)] {
                        io[base + offset..base + offset + 2].copy_from_slice(&value.to_le_bytes());
                    }
                    for axis in 0..2 {
                        let value = (i32::from(case) - 32) * 256;
                        let offset = base + 8 + axis * 4;
                        io[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                    }
                }
                let mut ppu = Ppu::default();
                for y in [0, 79, 159] {
                    ppu.render_line(y, &io, &palette, &vram, &oam);
                    for pixel in &ppu.pixels[y * 240..(y + 1) * 240] {
                        for byte in pixel.to_le_bytes() {
                            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                        }
                    }
                    for value in ppu.reference.iter().flatten() {
                        for byte in value.to_le_bytes() {
                            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                        }
                    }
                }
            }
            let expected = [
                0xfa24_0761_ce4d_62c7,
                0xf266_fbdb_baa6_d740,
                0x8a35_4798_6e30_b350,
                0x140c_c83d_0f15_ecd2,
                0x85f9_062f_23e5_f4a9,
                0x2c8e_3636_b5a4_02fb,
                0x78f5_ee7e_c52f_1913,
                0x06db_b9d3_b5a6_717e,
            ];
            assert_eq!(hash, expected[mode as usize], "mode={mode}");
        }
    }

    #[test]
    fn text_tile_rows_match_original_per_pixel_sampling() {
        fn original_color(
            x: usize,
            line: &super::BackgroundLine,
            palette: &[u8],
            vram: &[u8],
        ) -> Option<u16> {
            let x = x - x % line.mosaic_width;
            let sx = (x + line.scroll_x) % line.width;
            let sy = line.sy;
            let block = sx / 256 + (sy / 256) * (line.width / 256);
            let map = line.map_base + block * 0x800 + (((sy % 256) / 8) * 32 + (sx % 256) / 8) * 2;
            let entry = super::half(vram, map & 0xffff);
            let px = if entry & 0x400 != 0 {
                7 - sx % 8
            } else {
                sx % 8
            };
            let py = if entry & 0x800 != 0 {
                7 - sy % 8
            } else {
                sy % 8
            };
            let (index, bank) = if line.control & 0x80 != 0 {
                (
                    vram[(line.tile_base + (entry & 0x3ff) as usize * 64 + py * 8 + px) & 0xffff]
                        as usize,
                    0,
                )
            } else {
                let byte = vram
                    [(line.tile_base + (entry & 0x3ff) as usize * 32 + py * 4 + px / 2) & 0xffff];
                (
                    ((byte >> ((px & 1) * 4)) & 15) as usize,
                    (entry >> 12) as usize * 16,
                )
            };
            if index == 0 {
                None
            } else {
                Some(super::half(palette, (index + bank) * 2))
            }
        }

        let mut seed = 0x2567_4a19u32;
        let mut palette = [0; 0x400];
        let mut vram = vec![0; 0x18000];
        for bytes in [&mut palette[..], &mut vram[..]] {
            for byte in bytes {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *byte = (seed >> 24) as u8;
            }
        }
        let ppu = Ppu::default();
        let mut io = [0; 0x400];
        for size in 0..4 {
            for depth in [0, 0x80] {
                for base in 0..4 {
                    for map in [0, 31] {
                        let control: u16 = (size << 14) | depth | (base << 2) | (map << 8) | 0x40;
                        io[8..10].copy_from_slice(&control.to_le_bytes());
                        for scroll_x in [0u16, 1, 7, 8, 15, 248, 255, 256, 504, 511] {
                            io[0x10..0x12].copy_from_slice(&scroll_x.to_le_bytes());
                            for scroll_y in [0u16, 7, 255, 256, 511] {
                                io[0x12..0x14].copy_from_slice(&scroll_y.to_le_bytes());
                                for mosaic in [0, 0x32, 0xff] {
                                    io[0x4c] = mosaic;
                                    let line = super::BackgroundLine::new(0, 159, 0, &io);
                                    let mut row = super::TextTileRow::default();
                                    for x in 0..240 {
                                        // Emulate windows hiding spans, including skipped tile boundaries.
                                        if (x / 13) % 3 == 1 {
                                            continue;
                                        }
                                        assert_eq!(
                                            ppu.background(0, x, &line, &mut row, &palette, &vram)
                                                .map(|pixel| pixel.color),
                                            original_color(x, &line, &palette, &vram),
                                            "control={control:04x}, scroll=({scroll_x},{scroll_y}), mosaic={mosaic}, x={x}"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn text_tile_rows_are_rebuilt_after_vram_and_palette_writes() {
        let mut ppu = Ppu::default();
        let mut io = [0; 0x400];
        let mut palette = [0; 0x400];
        let mut vram = vec![0; 0x18000];
        io[1] = 1; // BG0 enabled, text mode.
        io[9] = 1; // Map at 0x800, tiles at zero.
        vram[0x800] = 1;
        vram[32..64].fill(0x11);
        palette[2..4].copy_from_slice(&31u16.to_le_bytes());
        palette[4..6].copy_from_slice(&0x7c00u16.to_le_bytes());
        ppu.render_line(0, &io, &palette, &vram, &[0; 0x400]);
        assert_eq!(ppu.pixels[0], Ppu::rgb(31));
        vram[32..64].fill(0x22);
        ppu.render_line(1, &io, &palette, &vram, &[0; 0x400]);
        assert_eq!(ppu.pixels[240], Ppu::rgb(0x7c00));
        palette[4..6].copy_from_slice(&0x03e0u16.to_le_bytes());
        ppu.render_line(2, &io, &palette, &vram, &[0; 0x400]);
        assert_eq!(ppu.pixels[480], Ppu::rgb(0x03e0));
    }

    #[test]
    fn decoded_windows_match_original_masks_at_boundaries() {
        // Retain the original per-pixel decoder as an independent reference.
        fn original_mask(x: usize, y: usize, io: &[u8], object: bool) -> u16 {
            let display = super::half(io, 0);
            if display & 0xe000 == 0 {
                return 0x3f;
            }
            for window in 0..2 {
                if display & (0x2000 << window) == 0 {
                    continue;
                }
                let horizontal = super::half(io, 0x40 + window * 2);
                let vertical = super::half(io, 0x44 + window * 2);
                if super::inside(x, (horizontal >> 8) as usize, (horizontal & 255) as usize)
                    && super::inside(y, (vertical >> 8) as usize, (vertical & 255) as usize)
                {
                    return (super::half(io, 0x48) >> (window * 8)) & 0x3f;
                }
            }
            if object && display & 0x8000 != 0 {
                super::half(io, 0x4a) >> 8 & 0x3f
            } else {
                super::half(io, 0x4a) & 0x3f
            }
        }

        let mut io = [0; 0x400];
        io[0x48..0x4c].copy_from_slice(&[0x12, 0x34, 0x25, 0x1a]);
        for start in [0, 1, 79, 159, 239, 240, 255] {
            for end in [0, 1, 79, 159, 239, 240, 255] {
                io[0x40..0x48].copy_from_slice(&[end, start, start, end, start, end, end, start]);
                for enabled in 0..8 {
                    io[1] = enabled << 5;
                    for y in [0, 79, 159, 255] {
                        let windows = super::WindowLine::new(y, super::half(&io, 0), &io);
                        for x in 0..256 {
                            for object in [false, true] {
                                assert_eq!(
                                    windows.mask(x, object),
                                    original_mask(x, y, &io, object),
                                    "start={start}, end={end}, enabled={enabled}, x={x}, y={y}, object={object}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn selecting_front_two_layers_matches_stable_full_sort_including_ties() {
        let mut seed = 0x1234_5678u32;
        for _ in 0..256 {
            let backdrop = super::Pixel {
                color: 0,
                layer: 5,
                priority: 4,
                semi: false,
            };
            let mut all = vec![backdrop];
            let mut top = backdrop;
            let mut below = None;
            for _ in 0..5 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let pixel = super::Pixel {
                    color: seed as u16,
                    layer: ((seed >> 16) % 6) as u8,
                    priority: ((seed >> 24) % 6) as u8,
                    semi: seed & 1 != 0,
                };
                all.push(pixel);
                let mut sorted = all.clone();
                sorted.sort_by_key(|pixel| {
                    (
                        pixel.priority,
                        if pixel.layer == 4 { 0 } else { pixel.layer + 1 },
                    )
                });
                super::Ppu::insert_layer(pixel, &mut top, &mut below);
                assert_eq!(top, sorted[0]);
                assert_eq!(below, Some(sorted[1]));
            }
        }
    }

    #[test]
    fn halfword_fast_path_preserves_all_in_range_and_wrapping_reads() {
        for length in [1, 2, 3, 1024, 0x18000] {
            let bytes: Vec<u8> = (0..length).map(|index| (index * 37) as u8).collect();
            for offset in 0..length * 2 + 4 {
                let expected = u16::from(bytes[offset % length])
                    | (u16::from(bytes[(offset + 1) % length]) << 8);
                assert_eq!(
                    super::half(&bytes, offset),
                    expected,
                    "len={length}, offset={offset}"
                );
            }
        }
    }

    #[test]
    fn semi_transparent_objects_blend_even_when_windows_disable_effects() {
        // Exercise WIN0, WIN1, OBJ window, and outside all windows. Ordinary
        // alpha/brightness effects must remain disabled in these regions.
        for region in 0..4 {
            for semi in [false, true] {
                for effect in 0..4 {
                    for second in [false, true] {
                        let mut ppu = Ppu::default();
                        let mut io = [0; 0x400];
                        let mut palette = [0; 0x400];
                        let mut vram = vec![0; 0x18000];
                        let mut oam = [0; 0x400];
                        let put = |bytes: &mut [u8], offset: usize, value: u16| {
                            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
                        };
                        let window = match region {
                            0 => 0x2000,
                            1 => 0x4000,
                            2 => 0x8000,
                            _ => 0x2000,
                        };
                        put(&mut io, 0, 0x1240 | window); // BG1, OBJ, 1D tiles.
                        put(&mut io, 0x0a, 0x0101); // BG1 map at 0x800, priority 1.
                        put(&mut io, 0x40, 8);
                        put(&mut io, 0x42, 8);
                        put(&mut io, 0x44, if region == 3 { 0 } else { 8 });
                        put(&mut io, 0x46, 8);
                        put(&mut io, 0x48, 0x1f1f);
                        put(&mut io, 0x4a, 0x1f1f);
                        // OBJ is deliberately not a first target: semi-transparent
                        // mode must force blending independently of BLDCNT too.
                        put(
                            &mut io,
                            0x50,
                            (effect << 6) | if second { 0x0200 } else { 0 },
                        );
                        put(&mut io, 0x52, 0x0808);
                        put(&mut io, 0x54, 16);
                        put(&mut palette, 2, 31); // Red BG.
                        put(&mut palette, 0x202, 31 << 10); // Blue OBJ.
                        vram[32..64].fill(0x11);
                        put(&mut vram, 0x800, 1);
                        vram[0x10000..0x10020].fill(0x11);
                        for object in 0..128 {
                            put(&mut oam, object * 8, 0x0200); // Disable unused OBJs.
                        }
                        put(&mut oam, 0, if semi { 0x0400 } else { 0 });
                        if region == 2 {
                            put(&mut oam, 8, 0x0800); // Overlapping OBJ window.
                        }
                        ppu.render_line(0, &io, &palette, &vram, &oam);
                        let expected = if semi && second {
                            15 | (15 << 10)
                        } else {
                            31 << 10
                        };
                        assert_eq!(
                            ppu.pixels[0],
                            Ppu::rgb(expected),
                            "region={region}, semi={semi}, effect={effect}, second={second}"
                        );
                        // Enabling OBJ as a first target must not enable ordinary
                        // effects where the window has disabled them.
                        io[0x50] |= 0x10;
                        ppu.render_line(0, &io, &palette, &vram, &oam);
                        assert_eq!(ppu.pixels[0], Ppu::rgb(expected));
                        // Window OBJ visibility is still honored for semi-transparent OBJs.
                        put(&mut io, 0x48, 0x0f0f);
                        put(&mut io, 0x4a, 0x0f0f);
                        ppu.render_line(0, &io, &palette, &vram, &oam);
                        assert_eq!(ppu.pixels[0], Ppu::rgb(31));
                    }
                }
            }
        }
    }

    #[test]
    fn bitmap_mode_three_renders_and_forced_blank_is_white() {
        let mut ppu = Ppu::default();
        let mut io = [0; 0x400];
        io[0] = 3;
        io[1] = 4;
        io[0x21] = 1;
        io[0x27] = 1;
        let mut vram = vec![0; 0x18000];
        vram[0] = 31;
        ppu.render_line(0, &io, &[0; 1024], &vram, &[0; 1024]);
        assert_eq!(ppu.pixels[0], 0xff0000);
        io[0] |= 0x80;
        ppu.render_line(0, &io, &[0; 1024], &vram, &[0; 1024]);
        assert_eq!(ppu.pixels[0], 0xffffff);
    }
}
