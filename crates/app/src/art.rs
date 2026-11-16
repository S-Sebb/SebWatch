//! Images drawn pixel by pixel: SebChat's picnic cloth and the file cache hatching.

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

fn rgba(rgb: u32, alpha: u8) -> Rgba8Pixel {
    Rgba8Pixel {
        r: (rgb >> 16) as u8,
        g: (rgb >> 8) as u8,
        b: rgb as u8,
        a: alpha,
    }
}

/// SebChat's picnic cloth (pc/src/look.rs): a 64-pixel tile, checkered in two halves, with three
/// flowers, at 3 image pixels per cloth pixel because tiling repeats an image at its own size.
pub fn cloth() -> Image {
    const PIXEL: usize = 3;
    let mut tile = [[0xFBE9D6; 64]; 64];
    for (y, row) in tile.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            if (x < 32) == (y < 32) {
                *cell = 0xF8E2CC;
            }
        }
    }
    for (x, y, petals, middle) in [
        (10, 12, 0xF4B6C2, 0xF0B443),
        (44, 50, 0xC9B6EF, 0xF0B443),
        (50, 16, 0xF4B6C2, 0xFFFFFF),
    ] {
        for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (0, 1)] {
            tile[(y + dy) as usize][(x + dx) as usize] = petals;
        }
        tile[y as usize][x as usize] = middle;
    }
    let size = 64 * PIXEL;
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(size as u32, size as u32);
    for (i, pixel) in buffer.make_mut_slice().iter_mut().enumerate() {
        *pixel = rgba(tile[i / size / PIXEL][i % size / PIXEL], 255);
    }
    Image::from_rgba8(buffer)
}

/// A 6-pixel tile of a rising diagonal stripe 1.5 pixels wide, as a white mask the theme colours.
pub fn hatch() -> Image {
    const SIZE: usize = 6;
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(SIZE as u32, SIZE as u32);
    for (i, pixel) in buffer.make_mut_slice().iter_mut().enumerate() {
        let alpha = match (i % SIZE + i / SIZE) % SIZE {
            0 => 255,
            1 => 128,
            _ => 0,
        };
        *pixel = rgba(0xFFFFFF, alpha);
    }
    Image::from_rgba8(buffer)
}
