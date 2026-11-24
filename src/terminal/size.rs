//! ピクセルサイズとセルサイズからターミナルのグリッドサイズを求める。

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::grid::Dimensions;

/// ターミナルのグリッドサイズ（列数・行数）とセルのピクセルサイズ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: f32,
    pub cell_height: f32,
}

impl GridSize {
    /// 描画領域のピクセルサイズを収まる限りのセル数に変換する（最低 1x1）。
    pub fn from_pixels(width: f32, height: f32, cell_width: f32, cell_height: f32) -> Self {
        Self {
            cols: cells_fitting(width, cell_width),
            rows: cells_fitting(height, cell_height),
            cell_width,
            cell_height,
        }
    }

    pub fn window_size(&self) -> WindowSize {
        WindowSize {
            num_lines: self.rows,
            num_cols: self.cols,
            cell_width: self.cell_width as u16,
            cell_height: self.cell_height as u16,
        }
    }
}

fn cells_fitting(length: f32, cell: f32) -> u16 {
    if !(cell > 0.0) || !length.is_finite() {
        return 1;
    }
    (length / cell).floor().clamp(1.0, u16::MAX as f32) as u16
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        self.rows as usize
    }

    fn columns(&self) -> usize {
        self.cols as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divides_pixels_by_cell_size() {
        let size = GridSize::from_pixels(800.0, 600.0, 10.0, 20.0);
        assert_eq!((size.cols, size.rows), (80, 30));
    }

    #[test]
    fn truncates_partial_cells() {
        let size = GridSize::from_pixels(805.0, 619.0, 10.0, 20.0);
        assert_eq!((size.cols, size.rows), (80, 30));
    }

    #[test]
    fn has_at_least_one_cell() {
        let size = GridSize::from_pixels(0.0, 5.0, 10.0, 20.0);
        assert_eq!((size.cols, size.rows), (1, 1));
    }

    #[test]
    fn ignores_invalid_cell_size() {
        let size = GridSize::from_pixels(800.0, 600.0, 0.0, f32::NAN);
        assert_eq!((size.cols, size.rows), (1, 1));
    }
}
