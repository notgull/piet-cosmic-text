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

use crate::text::Text;

use cosmic_text as ct;
use ct::{Buffer, LayoutRunIter};

use piet::kurbo::{Point, Rect, Size};
use piet::TextStorage;

use std::cmp;
use std::fmt;
use std::rc::Rc;

/// A text layout.
#[derive(Clone)]
pub struct TextLayout {
    /// The text buffer.
    text_buffer: Rc<BufferWrapper>,
}

impl fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextLayout")
            .field("string", &self.text_buffer.string.as_str())
            .field("glyph_size", &self.text_buffer.glyph_size)
            .finish_non_exhaustive()
    }
}

struct BufferWrapper {
    /// The original string.
    string: Box<dyn TextStorage>,

    /// The size of the glyph in pixels.
    glyph_size: i32,

    /// The original buffer.
    buffer: Option<Buffer>,

    /// Run metrics.
    run_metrics: Vec<piet::LineMetric>,

    /// The text handle.
    handle: Text,
}

impl BufferWrapper {
    fn buffer(&self) -> &Buffer {
        self.buffer.as_ref().unwrap()
    }
}

impl Drop for BufferWrapper {
    fn drop(&mut self) {
        let mut buffer = self.buffer.take().unwrap();
        buffer.lines.clear();
        let old_lines = self.handle.take_buffer();

        // Use whichever buffer has the most lines.
        if old_lines.capacity() > buffer.lines.capacity() {
            self.handle.set_buffer(old_lines);
        } else {
            self.handle.set_buffer(buffer.lines);
        }
    }
}

impl TextLayout {
    /// Create a new `TextLayout`.
    pub(crate) fn new(
        text: Text,
        buffer: Buffer,
        string: Box<dyn TextStorage>,
        glyph_size: i32,
    ) -> Self {
        // Figure out the metrics.
        let run_metrics = buffer
            .layout_runs()
            .map(|run| RunMetrics::new(run, glyph_size as f64))
            .map(|RunMetrics { line_metric }| line_metric)
            .collect();

        Self {
            text_buffer: Rc::new(BufferWrapper {
                string,
                glyph_size,
                buffer: Some(buffer),
                run_metrics,
                handle: text,
            }),
        }
    }

    /// Get a reference to the inner `Buffer`.
    pub fn buffer(&self) -> &Buffer {
        self.text_buffer.buffer()
    }

    /// Get an iterator over the layout runs.
    pub fn layout_runs(&self) -> LayoutRunIter<'_> {
        self.buffer().layout_runs()
    }
}

impl piet::TextLayout for TextLayout {
    fn size(&self) -> Size {
        self.layout_runs()
            .fold(Size::new(0.0, 0.0), |mut size, run| {
                let max_glyph_size = run
                    .glyphs
                    .iter()
                    .map(|glyph| glyph.font_size as i32)
                    .max()
                    .unwrap_or(self.text_buffer.glyph_size);

                let new_width = run.line_w as f64;
                if new_width > size.width {
                    size.width = new_width;
                }

                let new_height = (run.line_y as i32 + max_glyph_size) as f64;
                if new_height > size.height {
                    size.height = new_height;
                }

                size
            })
    }

    fn trailing_whitespace_width(&self) -> f64 {
        // TODO: This doesn't matter I think.
        self.size().width
    }

    fn image_bounds(&self) -> Rect {
        // TODO: Make this more exact.
        Rect::from_origin_size(Point::ZERO, self.size())
    }

    fn text(&self) -> &str {
        &self.text_buffer.string
    }

    fn line_text(&self, line_number: usize) -> Option<&str> {
        let run = self.buffer().layout_runs().nth(line_number)?;

        if run.glyphs.is_empty() {
            return None;
        }

        let start = run.glyphs[0].start;
        let end = run.glyphs.last().unwrap().end;

        Some(&self.text_buffer.string[start..end])
    }

    fn line_metric(&self, line_number: usize) -> Option<piet::LineMetric> {
        self.text_buffer.run_metrics.get(line_number).cloned()
    }

    fn line_count(&self) -> usize {
        self.buffer().layout_runs().count()
    }

    fn hit_test_point(&self, point: Point) -> piet::HitTestPoint {
        let mut htp = piet::HitTestPoint::default();
        let (x, y) = point.into();

        if let Some(cursor) = self.buffer().hit(x as f32, y as f32) {
            htp.idx = cursor.index;
            htp.is_inside = true;
            return htp;
        }

        // TODO
        htp
    }

    fn hit_test_text_position(&self, idx: usize) -> piet::HitTestPosition {
        // Iterator over glyphs and their assorted lines.
        let mut lines_and_glyphs = self.layout_runs().enumerate().flat_map(|(line, run)| {
            run.glyphs.iter().map(move |glyph| {
                (
                    line,
                    {
                        // Get the point.
                        let physical = glyph.physical((0.0, 0.0), 1.0);
                        let x = physical.x as f64;
                        let y = run.line_y as f64
                            + physical.y as f64
                            + self.text_buffer.glyph_size as f64;

                        Point::new(x, y)
                    },
                    glyph.start..glyph.end,
                )
            })
        });

        let (line, point, _) = match lines_and_glyphs.find(|(_, _, range)| range.contains(&idx)) {
            Some(x) => x,
            None => return piet::HitTestPosition::default(),
        };

        let mut htp = piet::HitTestPosition::default();
        htp.point = point;
        htp.line = line;
        htp
    }
}

/// Line metrics associated with a layout run.
struct RunMetrics {
    /// The `piet` line metrics.
    line_metric: piet::LineMetric,
}

impl RunMetrics {
    fn new(run: ct::LayoutRun<'_>, glyph_size: f64) -> RunMetrics {
        let (start_offset, end_offset) = run.glyphs.iter().fold((0, 0), |(start, end), glyph| {
            (cmp::min(start, glyph.start), cmp::max(end, glyph.end))
        });

        let y_offset = run.line_top.into();
        let baseline = run.line_y as f64 - run.line_top as f64;

        RunMetrics {
            line_metric: piet::LineMetric {
                start_offset,
                end_offset,
                trailing_whitespace: 0, // TODO
                y_offset,
                height: glyph_size as _,
                baseline,
            },
        }
    }
}
