// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
// This file is a part of `piet-cosmic-text`.
//
// `piet-cosmic-text` is free software: you can redistribute it and/or modify it under the
// terms of either:
//
// * GNU Lesser General Public License as published by the Free Software Foundation, either
//   version 3 of the License, or (at your option) any later version.
// * Mozilla Public License as published by the Mozilla Foundation, version 2.
//
// `piet-cosmic-text` is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR
// PURPOSE. See the GNU Lesser General Public License or the Mozilla Public License for more
// details.
//
// You should have received a copy of the GNU Lesser General Public License and the Mozilla
// Public License along with `piet-cosmic-text`. If not, see <https://www.gnu.org/licenses/>.

//! Integration with the [`line-straddler`] crate.
//!
//! [`line-straddler`]: https://crates.io/crates/line-straddler

use crate::metadata::Metadata;

use core::mem;
use cosmic_text::LayoutGlyph;
use line_straddler::{Glyph, GlyphStyle, Line as LsLine, LineGenerator, LineType};

use piet::kurbo::{Line, Point, Rect};
use piet::{Color, FontWeight};

/// A bundle between a line and a glyph styling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StyledLine {
    /// The line.
    pub line: Line,

    /// The color of the line.
    pub color: Color,

    /// Whether or not the line is bolded.
    pub bold: FontWeight,

    /// The size of the font, in pixels.
    pub font_size: f32,
}

impl StyledLine {
    /// Represent this styled line as a rectangle.
    pub fn into_rect(self) -> Rect {
        const FONT_WEIGHT_MULTIPLIER: f32 = 0.05;
        const OFFSET_MULTIPLIER: f32 = -0.83;

        let offset = self.font_size * OFFSET_MULTIPLIER;
        let width = self.font_size
            * (self.bold.to_raw() as f32 / FontWeight::NORMAL.to_raw() as f32)
            * FONT_WEIGHT_MULTIPLIER;

        let mut p0 = self.line.p0;
        let mut p1 = self.line.p1;
        p0.y += f64::from(offset);
        p1.y = p0.y - f64::from(width);
        Rect::from_points(p0, p1)
    }
}

/// State for text processing underlines and strikethroughs using [`line-straddler`].
///
/// [`line-straddler`]: https://crates.io/crates/line-straddler
#[derive(Debug)]
pub struct LineProcessor {
    /// State for the underline.
    underline: LineGenerator,

    /// State for the strikethrough.
    strikethrough: LineGenerator,

    /// The lines to draw.
    lines: Vec<StyledLine>,

    /// The last glyph size processed.
    last_glyph_size: f32,
}

impl Default for LineProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl LineProcessor {
    /// Create a new, empty state.
    pub fn new() -> Self {
        Self {
            underline: LineGenerator::new(LineType::Underline),
            strikethrough: LineGenerator::new(LineType::StrikeThrough),
            lines: Vec::new(),
            last_glyph_size: 0.0,
        }
    }

    /// Handle a glyph.
    pub fn handle_glyph(&mut self, glyph: &LayoutGlyph, line_y: f32, color: cosmic_text::Color) {
        // Get the metadata.
        let metadata = Metadata::from_raw(glyph.metadata);
        let font_size = glyph.font_size;
        let glyph = Glyph {
            line_y,
            font_size,
            width: glyph.w,
            x: glyph.x,
            style: GlyphStyle {
                boldness: metadata.boldness().to_raw(),
                color: match glyph.color_opt {
                    Some(color) => {
                        let [r, g, b, a] = [color.r(), color.g(), color.b(), color.a()];
                        line_straddler::Color::rgba(r, g, b, a)
                    }

                    None => {
                        let [r, g, b, a] = [color.r(), color.g(), color.b(), color.a()];
                        line_straddler::Color::rgba(r, g, b, a)
                    }
                },
            },
        };

        let Self {
            underline,
            strikethrough,
            lines,
            last_glyph_size,
        } = self;

        let handle_meta = |generator: &mut LineGenerator, has_it| {
            let line = if has_it {
                generator.add_glyph(glyph)
            } else {
                generator.pop_line()
            };

            line.map(|line| cvt_line(line, font_size))
        };

        let underline = handle_meta(underline, metadata.underline());
        let strikethrough = handle_meta(strikethrough, metadata.strikethrough());

        lines.extend(underline);
        lines.extend(strikethrough);
        *last_glyph_size = font_size;
    }

    /// Take the associated lines.
    pub fn lines(&mut self) -> Vec<StyledLine> {
        // Pop the last lines.
        let underline = self.underline.pop_line();
        let strikethrough = self.strikethrough.pop_line();
        let font_size = self.last_glyph_size;
        self.lines
            .extend(underline.map(|line| cvt_line(line, font_size)));
        self.lines
            .extend(strikethrough.map(|line| cvt_line(line, font_size)));

        mem::take(&mut self.lines)
    }
}

fn cvt_line(ls_line: LsLine, font_size: f32) -> StyledLine {
    let line = Line {
        p0: Point::new(ls_line.start_x.into(), ls_line.y.into()),
        p1: Point::new(ls_line.end_x.into(), ls_line.y.into()),
    };

    StyledLine {
        line,
        color: cvt_color(ls_line.style.color),
        bold: FontWeight::new(ls_line.style.boldness),
        font_size,
    }
}

fn cvt_color(color: line_straddler::Color) -> Color {
    let [r, g, b, a] = color.components();
    Color::rgba8(r, g, b, a)
}
