// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
// This file is a part of `piet-cosmic-text`.
//
// `piet-cosmic-text` is free software: you can redistribute it and/or modify it under the
// terms of either:
//
// * GNU Lesser General Public License as published by the Free Software Foundation, either
//   version 3 of the License, or (at your option) any later version.
// * Mozilla Public License as published by the Mozilla Foundation, version 2.
// * The Patron License (https://github.com/notgull/piet-cosmic-text/blob/main/LICENSE-PATRON.md)
//   for sponsors and contributors, who can ignore the copyleft provisions of the above licenses
//   for this project.
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

use crate::Metadata;
use core::mem;
use cosmic_text::LayoutGlyph;
use line_straddler::{Glyph, GlyphStyle, Line, LineGenerator, LineType};

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
    lines: Vec<piet::kurbo::Line>,
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
        }
    }

    /// Handle a glyph.
    pub fn handle_glyph(
        &mut self,
        glyph: &LayoutGlyph,
        line_y: f32,
        color: cosmic_text::Color,
        is_bold: bool,
    ) {
        // Get the metadata.
        let metadata = Metadata::from_raw(glyph.metadata);
        let glyph = Glyph {
            line_y,
            font_size: f32::from_bits(glyph.cache_key.font_size_bits),
            width: glyph.w,
            x: glyph.x,
            style: GlyphStyle {
                bold: is_bold,
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
        } = self;

        let handle_meta = |generator: &mut LineGenerator, has_it| {
            let line = if has_it {
                generator.add_glyph(glyph)
            } else {
                generator.pop_line()
            };

            line.map(cvt_line)
        };

        let underline = handle_meta(underline, metadata.underline());
        let strikethrough = handle_meta(strikethrough, metadata.strikethrough());

        lines.extend(underline);
        lines.extend(strikethrough);
    }

    /// Take the associated lines.
    pub fn lines(&mut self) -> Vec<piet::kurbo::Line> {
        // Pop the last lines.
        let underline = self.underline.pop_line();
        let strikethrough = self.strikethrough.pop_line();
        self.lines.extend(underline.map(cvt_line));
        self.lines.extend(strikethrough.map(cvt_line));

        mem::take(&mut self.lines)
    }
}

fn cvt_line(line: Line) -> piet::kurbo::Line {
    piet::kurbo::Line {
        p0: piet::kurbo::Point::new(line.start_x.into(), line.y.into()),
        p1: piet::kurbo::Point::new(line.end_x.into(), line.y.into()),
    }
}
