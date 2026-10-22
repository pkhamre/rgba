//! Software scanline renderer. Mid-scanline fetch/contention accuracy is pending.

#[derive(Clone, Copy, Debug)]
struct Pixel {
    color: u16,
    layer: u8,
    priority: u8,
    semi: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Ppu {
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
    u16::from(bytes[offset % bytes.len()]) | (u16::from(bytes[(offset + 1) % bytes.len()]) << 8)
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
            for x in 0..240 {
                let mask = Self::window_mask(x, y, io, object_window[x]);
                let mut layers = [backdrop; 6];
                let mut count = 1;
                for bg in 0..4 {
                    if display & (0x100 << bg) == 0 || mask & (1 << bg) == 0 {
                        continue;
                    }
                    if let Some(pixel) = self.background(bg, x, y, io, palette, vram) {
                        layers[count] = pixel;
                        count += 1;
                    }
                }
                if display & 0x1000 != 0
                    && mask & 0x10 != 0
                    && let Some(pixel) = objects[x]
                {
                    layers[count] = pixel;
                    count += 1;
                }
                layers[..count].sort_by_key(|pixel| {
                    (
                        pixel.priority,
                        if pixel.layer == 4 { 0 } else { pixel.layer + 1 },
                    )
                });
                let top = layers[0];
                let below = if count > 1 { layers[1] } else { backdrop };
                let mut color = top.color;
                let blend = half(io, 0x50);
                let first = blend & (1 << top.layer) != 0;
                let second = blend & (1 << (8 + below.layer)) != 0;
                let effect = (blend >> 6) & 3;
                if mask & 0x20 != 0 {
                    if (top.semi || (effect == 1 && first)) && second {
                        color = Self::mix(
                            top.color,
                            below.color,
                            (half(io, 0x52) & 31).min(16),
                            ((half(io, 0x52) >> 8) & 31).min(16),
                        );
                    } else if first && (effect == 2 || effect == 3) {
                        let amount = (half(io, 0x54) & 31).min(16);
                        color = Self::brightness(color, amount, effect == 2);
                    }
                }
                self.pixels[y * 240 + x] = Self::rgb(color);
            }
        }
        for bg in 0..2 {
            self.reference[bg][0] += i32::from(half(io, 0x22 + bg * 0x10) as i16);
            self.reference[bg][1] += i32::from(half(io, 0x26 + bg * 0x10) as i16);
        }
    }

    fn window_mask(x: usize, y: usize, io: &[u8], object: bool) -> u16 {
        let display = half(io, 0);
        if display & 0xe000 == 0 {
            return 0x3f;
        }
        for window in 0..2 {
            if display & (0x2000 << window) == 0 {
                continue;
            }
            let horizontal = half(io, 0x40 + window * 2);
            let vertical = half(io, 0x44 + window * 2);
            if inside(x, (horizontal >> 8) as usize, (horizontal & 255) as usize)
                && inside(y, (vertical >> 8) as usize, (vertical & 255) as usize)
            {
                return (half(io, 0x48) >> (window * 8)) & 0x3f;
            }
        }
        if object && display & 0x8000 != 0 {
            half(io, 0x4a) >> 8 & 0x3f
        } else {
            half(io, 0x4a) & 0x3f
        }
    }

    fn background(
        &self,
        bg: usize,
        mut x: usize,
        mut y: usize,
        io: &[u8],
        palette: &[u8],
        vram: &[u8],
    ) -> Option<Pixel> {
        let mode = half(io, 0) & 7;
        let control = half(io, 8 + bg * 2);
        if control & 0x40 != 0 {
            x -= x % ((io[0x4c] as usize & 15) + 1);
            y -= y % ((io[0x4c] as usize >> 4) + 1);
        }
        let text = mode == 0 || (mode == 1 && bg < 2);
        let affine = (mode == 1 && bg == 2) || (mode == 2 && bg >= 2);
        let color = if text {
            let size = (control >> 14) as usize;
            let width = if size & 1 != 0 { 512 } else { 256 };
            let height = if size & 2 != 0 { 512 } else { 256 };
            let sx = (x + (half(io, 0x10 + bg * 4) & 511) as usize) % width;
            let sy = (y + (half(io, 0x12 + bg * 4) & 511) as usize) % height;
            let block = sx / 256 + (sy / 256) * (width / 256);
            let map = ((control >> 8) & 31) as usize * 0x800
                + block * 0x800
                + (((sy % 256) / 8) * 32 + (sx % 256) / 8) * 2;
            let entry = half(vram, map & 0xffff);
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
            let base = ((control >> 2) & 3) as usize * 0x4000;
            let (index, bank) = if control & 0x80 != 0 {
                (
                    vram[(base + (entry & 0x3ff) as usize * 64 + py * 8 + px) & 0xffff] as usize,
                    0,
                )
            } else {
                let byte = vram[(base + (entry & 0x3ff) as usize * 32 + py * 4 + px / 2) & 0xffff];
                (
                    ((byte >> ((px & 1) * 4)) & 15) as usize,
                    (entry >> 12) as usize * 16,
                )
            };
            if index == 0 {
                return None;
            }
            half(palette, (index + bank) * 2)
        } else if affine || ((3..=5).contains(&mode) && bg == 2) {
            let affine_index = bg - 2;
            let pa = i32::from(half(io, 0x20 + affine_index * 0x10) as i16);
            let pc = i32::from(half(io, 0x24 + affine_index * 0x10) as i16);
            let mut sx = (self.reference[affine_index][0] + pa * x as i32) >> 8;
            let mut sy = (self.reference[affine_index][1] + pc * x as i32) >> 8;
            if affine {
                let size = 128 << (control >> 14);
                if control & 0x2000 != 0 {
                    sx = sx.rem_euclid(size);
                    sy = sy.rem_euclid(size);
                } else if sx < 0 || sy < 0 || sx >= size || sy >= size {
                    return None;
                }
                let map = ((control >> 8) & 31) as usize * 0x800;
                let tile = vram
                    [(map + (sy as usize / 8) * (size as usize / 8) + sx as usize / 8) & 0xffff]
                    as usize;
                let base = ((control >> 2) & 3) as usize * 0x4000;
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
                let page = if mode != 3 && half(io, 0) & 0x10 != 0 {
                    0xa000
                } else {
                    0
                };
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
