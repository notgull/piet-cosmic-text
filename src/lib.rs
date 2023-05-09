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

//! An implementation of [`piet`]'s text API using [`cosmic-text`].
//!
//! This library implements [`piet`]'s [`Text`] API using primitives from [`cosmic-text`].
//! The intention is for this library to act as a stepping stone to be able to use drawing
//! frameworks that do not natively support text rendering (like OpenGL) by using the
//! [`cosmic-text`] library to render text to a texture, and then using that texture
//! in the drawing framework.
//!
//! This library provides a [`Text`](crate::Text), a [`TextLayoutBuilder`] and a
//! [`TextLayout`]. All of these are intended to be used in the same way as the
//! corresponding types in [`piet`]. However, [`TextLayout`] has a `buffer` method that
//! can be used to get the underlying text.
//!
//! # Limitations
//!
//! - New fonts cannot be loaded while a [`TextLayout`] is alive.
//! - The text does not support [`TextAlignment`] or variable font sizes. Attempting to
//!   use these will result in an error.
//!
//! [`piet`]: https://docs.rs/piet
//! [`cosmic-text`]: https://docs.rs/cosmic-text
//! [`TextAlignment`]: https://docs.rs/piet/latest/piet/enum.TextAlignment.html

#![allow(clippy::await_holding_refcell_ref)]
#![forbid(unsafe_code, future_incompatible, rust_2018_idioms)]

use cosmic_text::fontdb::Family;
use cosmic_text::{Attrs, AttrsList, Buffer, BufferLine, FontSystem, LayoutRunIter, Metrics};

use piet::kurbo::{Point, Rect, Size};
use piet::{util, Error, FontFamily, TextAlignment, TextAttribute, TextStorage};

use std::cell::{Cell, RefCell};
use std::cmp;
use std::fmt;
use std::ops::{Range, RangeBounds};
use std::rc::Rc;

/// The metadata stored in the font's stylings.
///
/// This should be considered by the renderer in order to render extra decorations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Metadata(usize);

const UNDERLINE: usize = 1 << 0;
const STRIKETHROUGH: usize = 1 << 1;

impl Metadata {
    /// Create a new, empty metadata.
    pub fn new() -> Self {
        Self(0)
    }

    /// Create a metadata from the raw value.
    pub fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    /// Convert into the raw value.
    pub fn into_raw(self) -> usize {
        self.0
    }

    /// Set the "underline" bit.
    pub fn set_underline(&mut self, underline: bool) {
        if underline {
            self.0 |= UNDERLINE;
        } else {
            self.0 &= !UNDERLINE;
        }
    }

    /// Set the "strikethrough" bit.
    pub fn set_strikethrough(&mut self, strikethrough: bool) {
        if strikethrough {
            self.0 |= STRIKETHROUGH;
        } else {
            self.0 &= !STRIKETHROUGH;
        }
    }

    /// Is the "underline" bit set?
    pub fn underline(&self) -> bool {
        self.0 & UNDERLINE != 0
    }

    /// Is the "strikethrough" bit set?
    pub fn strikethrough(&self) -> bool {
        self.0 & STRIKETHROUGH != 0
    }
}

/// The text implementation entry point.
///
/// # Limitations
///
/// `load_from_data` should not be called while `TextLayout` objects are alive; otherwise, a panic will
/// occur.
#[derive(Clone)]
pub struct Text(Rc<Inner>);

struct Inner {
    /// Font database.
    font_db: RefCell<FontSystem>,

    /// Buffer that holds lines of text.
    ///
    /// These are held here so that they aren't constantly reallocated.
    buffer: Cell<Vec<BufferLine>>,
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Text { .. }")
    }
}

impl Text {
    /// Create a new `Text` renderer.
    pub fn new() -> Self {
        Self::from_font_system(FontSystem::new())
    }

    /// Create a new `Text` renderer from a `FontSystem`.
    pub fn from_font_system(font_system: FontSystem) -> Self {
        Self(Rc::new(Inner {
            font_db: RefCell::new(font_system),
            buffer: Cell::new(Vec::new()),
        }))
    }

    /// Run a closure with access to the underlying `FontSystem`.
    ///
    /// # Notes
    ///
    /// Loading new fonts while this function is in use will result in an error.
    pub fn with_font_system<R>(&self, f: impl FnOnce(&FontSystem) -> R) -> R {
        let font_db = self.0.font_db.borrow();
        f(&font_db)
    }

    /// Run a closure with mutable access to the underlying `FontSystem`.
    ///
    /// # Notes
    ///
    /// Loading new fonts while this function is in use will result in an error.
    pub fn with_font_system_mut<R>(&self, f: impl FnOnce(&mut FontSystem) -> R) -> R {
        let mut font_db = self.0.font_db.borrow_mut();
        f(&mut font_db)
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}

impl piet::Text for Text {
    type TextLayout = TextLayout;
    type TextLayoutBuilder = TextLayoutBuilder;

    fn font_family(&mut self, family_name: &str) -> Option<FontFamily> {
        let db = self.0.font_db.try_borrow().ok()?;

        // Get the font family.
        let family = Family::Name(family_name);
        let name = db.db().family_name(&family);

        // Look for the font with that name.
        let x = db
            .db()
            .faces()
            .flat_map(|face| &face.families)
            .find(|(face, _)| *face == name)
            .map(|(face, _)| FontFamily::new_unchecked(face.clone()));

        x
    }

    fn load_font(&mut self, data: &[u8]) -> Result<FontFamily, Error> {
        let font_name = font_name(data)?;

        // Fast path: try to load the font by its name.
        if let Some(family) = self.font_family(&font_name) {
            return Ok(family);
        }

        // Slow path: insert the font into the database and then try to load it by its name.
        {
            let mut db = self
                .0
                .font_db
                .try_borrow_mut()
                .map_err(|_| Error::FontLoadingFailed)?;
            db.db_mut().load_font_data(data.into());
        }

        self.font_family(&font_name)
            .ok_or_else(|| Error::FontLoadingFailed)
    }

    fn new_text_layout(&mut self, text: impl TextStorage) -> Self::TextLayoutBuilder {
        let text = Rc::new(text);

        TextLayoutBuilder {
            handle: self.clone(),
            string: text,
            defaults: util::LayoutDefaults::default(),
            range_attributes: vec![],
            last_range_start_pos: 0,
            max_width: f64::INFINITY,
            error: None,
        }
    }
}

/// The text layout builder used by the [`Text`].
pub struct TextLayoutBuilder {
    /// Handle to the original `Text` object.
    handle: Text,

    /// The string we're laying out.
    string: Rc<dyn TextStorage>,

    /// The default text attributes.
    defaults: util::LayoutDefaults,

    /// The width constraint.
    max_width: f64,

    /// The range attributes.
    range_attributes: Vec<(Range<usize>, TextAttribute)>,
    last_range_start_pos: usize,

    /// The last error that occurred.
    error: Option<Error>,
}

impl fmt::Debug for TextLayoutBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextLayoutBuilder")
            .field("string", &self.string.as_str())
            .field("max_width", &self.max_width)
            .field("range_attributes", &self.range_attributes)
            .finish_non_exhaustive()
    }
}

impl piet::TextLayoutBuilder for TextLayoutBuilder {
    type Out = TextLayout;

    fn alignment(mut self, alignment: TextAlignment) -> Self {
        // TODO: Support alignment.
        if !matches!(alignment, TextAlignment::Start) {
            self.error = Some(Error::NotSupported);
        }
        self
    }

    fn max_width(mut self, width: f64) -> Self {
        self.max_width = width;
        self
    }

    fn default_attribute(mut self, attribute: impl Into<TextAttribute>) -> Self {
        self.defaults.set(attribute);
        self
    }

    fn range_attribute(
        mut self,
        range: impl RangeBounds<usize>,
        attribute: impl Into<TextAttribute>,
    ) -> Self {
        let range = util::resolve_range(range, self.string.len());
        let attribute = attribute.into();

        debug_assert!(
            range.start >= self.last_range_start_pos,
            "attributes must be added in non-decreasing start order"
        );
        self.last_range_start_pos = range.start;

        self.range_attributes.push((range, attribute));

        self
    }

    fn build(self) -> Result<Self::Out, Error> {
        let Self {
            handle,
            string,
            defaults,
            max_width,
            range_attributes,
            error,
            ..
        } = self;

        // If an error occurred, return it.
        if let Some(error) = error {
            return Err(error);
        }

        // Get the font size and line height.
        let font_size = points_to_pixels(defaults.font_size);

        // NOTE: Pango uses a default line height of 0, and piet-cairo doesn't appear to
        // change this.
        let metrics = Metrics::new(font_size as _, font_size as _);

        // Get the default attributes for the layout.
        let default_attrs = {
            let mut metadata = Metadata::new();

            if defaults.underline {
                metadata.set_underline(true);
            }

            if defaults.strikethrough {
                metadata.set_strikethrough(true);
            }

            let mut attrs = Attrs::new()
                .family(cvt_family(&defaults.font))
                .weight(cvt_weight(defaults.weight))
                .style(cvt_style(defaults.style))
                .metadata(metadata.into_raw());

            if defaults.fg_color != util::DEFAULT_TEXT_COLOR {
                attrs = attrs.color(cvt_color(defaults.fg_color));
            }

            attrs
        };

        // Re-use memory from a previous layout.
        let mut buffer_lines = handle.0.buffer.take();
        let mut offset = 0;

        for line in string.lines() {
            let start = offset;
            let end = start + line.len() + 1;

            // Get the attributes for this line.
            let mut attrs_list = AttrsList::new(default_attrs);

            // TODO: This algorithm is quadratic time, use something more efficient.
            for (range, alg) in &range_attributes {
                if let Some(range) = intersect_ranges(range, &(start..end)) {
                    let range = range.start - start..range.end - start;

                    match alg {
                        TextAttribute::FontFamily(family) => {
                            attrs_list.add_span(range, default_attrs.family(cvt_family(family)));
                        }
                        TextAttribute::FontSize(_) => {
                            // TODO: Implement variable font sizes.
                            return Err(Error::Unimplemented);
                        }
                        TextAttribute::Weight(weight) => {
                            attrs_list.add_span(range, default_attrs.weight(cvt_weight(*weight)));
                        }
                        TextAttribute::Style(style) => {
                            attrs_list.add_span(range, default_attrs.style(cvt_style(*style)));
                        }
                        TextAttribute::TextColor(color) => {
                            if *color != util::DEFAULT_TEXT_COLOR {
                                attrs_list.add_span(range, default_attrs.color(cvt_color(*color)));
                            }
                        }
                        TextAttribute::Underline(_) => {
                            attrs_list.add_span(
                                range,
                                default_attrs.metadata({
                                    let mut metadata = Metadata::new();
                                    metadata.set_underline(true);
                                    metadata.into_raw()
                                }),
                            );
                        }
                        TextAttribute::Strikethrough(_) => {
                            attrs_list.add_span(
                                range,
                                default_attrs.metadata({
                                    let mut metadata = Metadata::new();
                                    metadata.set_strikethrough(true);
                                    metadata.into_raw()
                                }),
                            );
                        }
                    }
                }
            }

            buffer_lines.push(BufferLine::new(line, attrs_list));

            offset = end;
        }

        let buffer = {
            let mut font_system = handle.0.font_db.borrow_mut();
            let mut buffer = Buffer::new(&mut font_system, metrics);

            buffer.lines = buffer_lines;
            buffer.set_size(&mut font_system, max_width as f32, f32::INFINITY);
            buffer.set_wrap(&mut font_system, cosmic_text::Wrap::Word);

            // Shape the buffer.
            buffer.shape_until_scroll(&mut font_system);

            buffer
        };

        Ok(TextLayout {
            string,
            glyph_size: font_size as i32,
            text_buffer: Rc::new(BufferWrapper {
                buffer: Some(buffer),
                handle,
            }),
        })
    }
}

/// A text layout.
#[derive(Clone)]
pub struct TextLayout {
    /// The original string.
    string: Rc<dyn TextStorage>,

    /// The size of the glyph in pixels.
    glyph_size: i32,

    /// The text buffer.
    text_buffer: Rc<BufferWrapper>,
}

impl fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextLayout")
            .field("string", &self.string.as_str())
            .field("glyph_size", &self.glyph_size)
            .finish_non_exhaustive()
    }
}

struct BufferWrapper {
    /// The original buffer.
    buffer: Option<Buffer>,

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
        let old_lines = self.handle.0.buffer.take();

        // Use whichever buffer has the most lines.
        if old_lines.capacity() > buffer.lines.capacity() {
            self.handle.0.buffer.set(old_lines);
        } else {
            self.handle.0.buffer.set(buffer.lines);
        }
    }
}

impl TextLayout {
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
                    .map(|glyph| f32::from_bits(glyph.cache_key.font_size_bits) as i32)
                    .max()
                    .unwrap_or(self.glyph_size);

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
        &self.string
    }

    fn line_text(&self, line_number: usize) -> Option<&str> {
        let run = self.buffer().layout_runs().nth(line_number)?;

        if run.glyphs.is_empty() {
            return None;
        }

        let start = run.glyphs[0].start;
        let end = run.glyphs.last().unwrap().end;

        Some(&self.string[start..end])
    }

    fn line_metric(&self, line_number: usize) -> Option<piet::LineMetric> {
        self.layout_runs().nth(line_number).map(|run| {
            let (start, end) = run.glyphs.iter().fold((0, 0), |(start, end), glyph| {
                (cmp::min(start, glyph.start), cmp::max(end, glyph.end))
            });

            piet::LineMetric {
                start_offset: start,
                end_offset: end,
                trailing_whitespace: 0, // TODO
                y_offset: run.line_y as _,
                height: self.glyph_size as _,
                baseline: run.line_y as f64 + self.glyph_size as f64,
            }
        })
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
                        let x = glyph.x_int as f64;
                        let y = run.line_y as f64 + glyph.y_int as f64 + self.glyph_size as f64;

                        Point::new(x, y)
                    },
                    glyph.start..glyph.end,
                )
            })
        });

        let (line, point, _) = lines_and_glyphs
            .find(|(_, _, range)| range.contains(&idx))
            .expect("Index out of bounds.");

        let mut htp = piet::HitTestPosition::default();
        htp.point = point;
        htp.line = line;
        htp
    }
}

/// Intersection of two ranges.
fn intersect_ranges(a: &Range<usize>, b: &Range<usize>) -> Option<Range<usize>> {
    let start = a.start.max(b.start);
    let end = a.end.min(b.end);

    if start < end {
        Some(start..end)
    } else {
        None
    }
}

fn font_name(font: &[u8]) -> Result<String, Error> {
    // Parse it using ttf-parser
    let font = ttf_parser::Face::parse(font, 0).map_err(|e| Error::BackendError(e.into()))?;

    // Get the name with the main ID.
    let name = font
        .names()
        .into_iter()
        .find(|n| n.name_id == ttf_parser::name_id::FULL_NAME && n.is_unicode())
        .ok_or_else(|| Error::BackendError("font does not have a name with the main ID".into()))?;

    // TODO: Support macintosh encoding.
    name.to_string()
        .ok_or_else(|| Error::BackendError("font name is not valid UTF-16".into()))
}

fn points_to_pixels(points: f64) -> f64 {
    points * 96.0 / 72.0
}

fn cvt_color(p: piet::Color) -> cosmic_text::Color {
    let (r, g, b, a) = p.as_rgba8();
    cosmic_text::Color::rgba(r, g, b, a)
}

fn cvt_family(p: &piet::FontFamily) -> cosmic_text::Family<'_> {
    macro_rules! generic {
        ($piet:ident => $cosmic:ident) => {
            if p == &piet::FontFamily::$piet {
                return cosmic_text::Family::$cosmic;
            }
        };
    }

    if p.is_generic() {
        generic!(SERIF => Serif);
        generic!(SANS_SERIF => SansSerif);
        generic!(MONOSPACE => Monospace);
    }

    cosmic_text::Family::Name(p.name())
}

fn cvt_style(p: piet::FontStyle) -> cosmic_text::Style {
    use piet::FontStyle;

    match p {
        FontStyle::Italic => cosmic_text::Style::Italic,
        FontStyle::Regular => cosmic_text::Style::Normal,
    }
}

fn cvt_weight(p: piet::FontWeight) -> cosmic_text::Weight {
    cosmic_text::Weight(p.to_raw())
}
