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

use piet::kurbo::{Point, Rect, Size, Vec2};
use piet::TextStorage;

use swash::scale::image::Image as SwashImage;
use swash::scale::outline::Outline as SwashOutline;
use swash::scale::{ScaleContext, StrikeWith};
use swash::zeno;

use std::cell::Cell;
use std::cmp;
use std::collections::hash_map::{Entry, HashMap};
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

    /// Ink rectangle for the buffer.
    ink_rectangle: Rect,

    /// Logical extent for the buffer.
    logical_size: Cell<Option<Size>>,

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
        font_system: &mut ct::FontSystem,
    ) -> Self {
        let span = trace_span!("TextLayout::new", string = %string.as_str());
        let _guard = span.enter();

        // Figure out the metrics.
        let run_metrics = buffer
            .layout_runs()
            .map(|run| RunMetrics::new(run, glyph_size as f64))
            .map(|RunMetrics { line_metric }| line_metric)
            .collect();

        // Scale up the buffers to get a good idea of the ink rectangle.
        let mut ink_context = text.borrow_ink();
        let mut missing_bbox_count = 0;

        let bounding_boxes = buffer
            .layout_runs()
            .flat_map(|run| {
                let run_y = run.line_y;
                run.glyphs.iter().map(move |glyph| (glyph, run_y))
            })
            .filter_map(|(glyph, run_y)| {
                let physical = glyph.physical((0., 0.), 1.);
                let offset = Vec2::new(
                    physical.x as f64 + physical.cache_key.x_bin.as_float() as f64,
                    run_y as f64 + physical.y as f64 + physical.cache_key.y_bin.as_float() as f64,
                );

                // Figure out the bounding box.
                match ink_context.bounding_box(&physical, font_system) {
                    Some(mut rect) => {
                        rect = rect + offset;
                        Some(rect)
                    }

                    None => {
                        missing_bbox_count += 1;
                        None
                    }
                }
            });
        let ink_rectangle = bounding_rectangle(bounding_boxes);

        if missing_bbox_count > 0 {
            warn!("Missing {} bounding boxes", missing_bbox_count);
        }

        drop(ink_context);

        Self {
            text_buffer: Rc::new(BufferWrapper {
                string,
                glyph_size,
                buffer: Some(buffer),
                run_metrics,
                handle: text,
                ink_rectangle,
                logical_size: Cell::new(None),
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
        if let Some(size) = self.text_buffer.logical_size.get() {
            return size;
        }

        let mut size = Size::new(f64::MIN, f64::MIN);

        for run in self.layout_runs() {
            let max = |a: f32, b: f64| {
                let a: f64 = a.into();
                if a < b {
                    b
                } else {
                    a
                }
            };

            size.width = max(run.line_w, size.width);
            size.height = max(run.line_y, size.height);
        }

        self.text_buffer.logical_size.set(Some(size));

        size
    }

    fn trailing_whitespace_width(&self) -> f64 {
        // TODO: This doesn't matter I think.
        self.size().width
    }

    fn image_bounds(&self) -> Rect {
        self.text_buffer.ink_rectangle
    }

    fn text(&self) -> &str {
        &self.text_buffer.string
    }

    fn line_text(&self, line_number: usize) -> Option<&str> {
        self.buffer()
            .layout_runs()
            .nth(line_number)
            .map(|run| run.text)
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

        let mut ink_context = self.text_buffer.handle.borrow_ink();
        let mut font_system_guard = match self.text_buffer.handle.borrow_font_system() {
            Some(system) => system,
            None => {
                warn!("Tried to borrow font system to calculate better hit test point, but it was already borrowed.");
                htp.idx = 0;
                htp.is_inside = false;
                return htp;
            }
        };
        let font_system = &mut font_system_guard
            .get()
            .expect("For a TextLayout to exist, the font system must have already been initialized")
            .system;

        // Look for the glyph with the closest distance to the point.
        let mut closest_distance = f64::MAX;

        for (glyph, physical_glyph) in self.layout_runs().flat_map(|run| {
            let run_y = run.line_y;
            run.glyphs
                .iter()
                .map(move |glyph| (glyph, glyph.physical((0., run_y), 1.)))
        }) {
            let bounding_box = match ink_context.bounding_box(&physical_glyph, font_system) {
                Some(bbox) => bbox,
                None => continue,
            };

            // If the point is inside of the bounding box, this is definitely it.
            if bounding_box.contains(point) {
                htp.idx = glyph.start;
                htp.is_inside = false;
                return htp;
            }

            // Otherwise, find the distance from the midpoint.
            let midpoint = bounding_box.center();
            let distance = midpoint.distance(point);
            if distance < closest_distance {
                closest_distance = distance;
                htp.idx = glyph.start;
            }
        }

        // If we didn't find anything, just return the closest index.
        htp.is_inside = false;
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
            None => {
                // TODO: What are you supposed to do here?
                return piet::HitTestPosition::default();
            }
        };

        let mut htp = piet::HitTestPosition::default();
        htp.point = point;
        htp.line = line;
        htp
    }
}

fn bounding_rectangle(rects: impl IntoIterator<Item = Rect>) -> Rect {
    let mut iter = rects.into_iter();
    let mut sum_rect = match iter.next() {
        Some(rect) => rect,
        None => return Rect::ZERO,
    };

    for rect in iter {
        if rect.x0 < sum_rect.x0 {
            sum_rect.x0 = rect.x0;
        }
        if rect.y0 < sum_rect.y0 {
            sum_rect.y0 = rect.y0;
        }
        if rect.x1 > sum_rect.x1 {
            sum_rect.x1 = rect.x1;
        }
        if rect.y1 > sum_rect.y1 {
            sum_rect.y1 = rect.y1;
        }
    }

    sum_rect
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

/// State for calculating the ink rectangle.
pub(crate) struct InkRectangleState {
    /// The swash scaling context.
    scaler: ScaleContext,

    /// Cache between fonts, glyphs and their bounding boxes.
    bbox_cache: HashMap<ct::CacheKey, Option<Rect>>,

    /// Swash image buffer.
    swash_image: SwashImage,

    /// Swash outline buffer.
    swash_outline: SwashOutline,
}

impl InkRectangleState {
    pub(crate) fn new() -> Self {
        Self {
            scaler: ScaleContext::new(),
            bbox_cache: HashMap::new(),
            swash_image: SwashImage::new(),
            swash_outline: SwashOutline::new(),
        }
    }

    /// Get the bounding box for a glyph.
    fn bounding_box(
        &mut self,
        glyph: &ct::PhysicalGlyph,
        system: &mut ct::FontSystem,
    ) -> Option<Rect> {
        // If we already have the bounding box here, return it.
        let entry = match self.bbox_cache.entry(glyph.cache_key) {
            Entry::Occupied(o) => return *o.into_mut(),
            Entry::Vacant(v) => v,
        };

        let mut bbox = None;

        // Find the font.
        if let Some(font) = system.get_font(glyph.cache_key.font_id) {
            // Create a scaler for this font.
            let mut scaler = self
                .scaler
                .builder(font.as_swash())
                .size(f32::from_bits(glyph.cache_key.font_size_bits))
                .build();

            // See if we can get an outline.
            self.swash_outline.clear();
            if scaler.scale_outline_into(glyph.cache_key.glyph_id, &mut self.swash_outline) {
                bbox = Some(cvt_bounds(self.swash_outline.bounds()));
            } else {
                // See if we can get a bitmap.
                self.swash_image.clear();
                if scaler.scale_bitmap_into(
                    glyph.cache_key.glyph_id,
                    StrikeWith::BestFit,
                    &mut self.swash_image,
                ) {
                    bbox = Some(cvt_placement(self.swash_image.placement));
                }
            }
        }

        // Cache the result.
        *entry.insert(bbox)
    }
}

fn cvt_placement(placement: zeno::Placement) -> Rect {
    Rect::new(
        placement.left.into(),
        -placement.top as f64,
        placement.left as f64 + placement.width as f64,
        -placement.top as f64 + placement.height as f64,
    )
}

fn cvt_bounds(mut bounds: zeno::Bounds) -> Rect {
    bounds.min.y *= -1.0;
    bounds.max.y *= -1.0;
    Rect::from_points(cvt_point(bounds.min), cvt_point(bounds.max))
}

fn cvt_point(point: zeno::Point) -> Point {
    Point::new(point.x.into(), point.y.into())
}
