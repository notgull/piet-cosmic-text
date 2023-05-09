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
        .find(|n| n.name_id == ttf_parser::name_id::FAMILY)
        .ok_or_else(|| Error::BackendError("font does not have a name with the main ID".into()))?;

    // Try to convert to a string.
    name.to_string()
        .or_else(|| {
            // See if the name is in Macintosh encoding.
            if name.platform_id == ttf_parser::PlatformId::Macintosh && name.encoding_id == 0 {
                // Translate the Macintosh encoding to UTF-16 and then parse it.
                String::from_utf16(
                    &name
                        .name
                        .iter()
                        .map(|x| MAC_ROMAN[*x as usize])
                        .collect::<Vec<_>>(),
                )
                .ok()
            } else {
                None
            }
        })
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

/// Macintosh Roman to UTF-16 encoding table.
/// 
/// Taken from here:
/// https://github.com/RazrFalcon/fontdb/blob/c07be3aa11d4efc957cd5fb6560da4f54c3fea6f/src/lib.rs#L1229-L1266
/// Originally licensed under the MIT License, reproduced in full below:
/// 
/// The MIT License (MIT)
/// 
/// Copyright (c) 2020 Yevhenii Reizner
/// 
/// Permission is hereby granted, free of charge, to any person obtaining a copy
/// of this software and associated documentation files (the "Software"), to deal
/// in the Software without restriction, including without limitation the rights
/// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
/// copies of the Software, and to permit persons to whom the Software is
/// furnished to do so, subject to the following conditions:
/// 
/// The above copyright notice and this permission notice shall be included in all
/// copies or substantial portions of the Software.
/// 
/// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
/// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
/// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
/// SOFTWARE.
///
/// https://en.wikipedia.org/wiki/Mac_OS_Roman
#[rustfmt::skip]
const MAC_ROMAN: &[u16; 256] = &[
    0x0000, 0x0001, 0x0002, 0x0003, 0x0004, 0x0005, 0x0006, 0x0007,
    0x0008, 0x0009, 0x000A, 0x000B, 0x000C, 0x000D, 0x000E, 0x000F,
    0x0010, 0x2318, 0x21E7, 0x2325, 0x2303, 0x0015, 0x0016, 0x0017,
    0x0018, 0x0019, 0x001A, 0x001B, 0x001C, 0x001D, 0x001E, 0x001F,
    0x0020, 0x0021, 0x0022, 0x0023, 0x0024, 0x0025, 0x0026, 0x0027,
    0x0028, 0x0029, 0x002A, 0x002B, 0x002C, 0x002D, 0x002E, 0x002F,
    0x0030, 0x0031, 0x0032, 0x0033, 0x0034, 0x0035, 0x0036, 0x0037,
    0x0038, 0x0039, 0x003A, 0x003B, 0x003C, 0x003D, 0x003E, 0x003F,
    0x0040, 0x0041, 0x0042, 0x0043, 0x0044, 0x0045, 0x0046, 0x0047,
    0x0048, 0x0049, 0x004A, 0x004B, 0x004C, 0x004D, 0x004E, 0x004F,
    0x0050, 0x0051, 0x0052, 0x0053, 0x0054, 0x0055, 0x0056, 0x0057,
    0x0058, 0x0059, 0x005A, 0x005B, 0x005C, 0x005D, 0x005E, 0x005F,
    0x0060, 0x0061, 0x0062, 0x0063, 0x0064, 0x0065, 0x0066, 0x0067,
    0x0068, 0x0069, 0x006A, 0x006B, 0x006C, 0x006D, 0x006E, 0x006F,
    0x0070, 0x0071, 0x0072, 0x0073, 0x0074, 0x0075, 0x0076, 0x0077,
    0x0078, 0x0079, 0x007A, 0x007B, 0x007C, 0x007D, 0x007E, 0x007F,
    0x00C4, 0x00C5, 0x00C7, 0x00C9, 0x00D1, 0x00D6, 0x00DC, 0x00E1,
    0x00E0, 0x00E2, 0x00E4, 0x00E3, 0x00E5, 0x00E7, 0x00E9, 0x00E8,
    0x00EA, 0x00EB, 0x00ED, 0x00EC, 0x00EE, 0x00EF, 0x00F1, 0x00F3,
    0x00F2, 0x00F4, 0x00F6, 0x00F5, 0x00FA, 0x00F9, 0x00FB, 0x00FC,
    0x2020, 0x00B0, 0x00A2, 0x00A3, 0x00A7, 0x2022, 0x00B6, 0x00DF,
    0x00AE, 0x00A9, 0x2122, 0x00B4, 0x00A8, 0x2260, 0x00C6, 0x00D8,
    0x221E, 0x00B1, 0x2264, 0x2265, 0x00A5, 0x00B5, 0x2202, 0x2211,
    0x220F, 0x03C0, 0x222B, 0x00AA, 0x00BA, 0x03A9, 0x00E6, 0x00F8,
    0x00BF, 0x00A1, 0x00AC, 0x221A, 0x0192, 0x2248, 0x2206, 0x00AB,
    0x00BB, 0x2026, 0x00A0, 0x00C0, 0x00C3, 0x00D5, 0x0152, 0x0153,
    0x2013, 0x2014, 0x201C, 0x201D, 0x2018, 0x2019, 0x00F7, 0x25CA,
    0x00FF, 0x0178, 0x2044, 0x20AC, 0x2039, 0x203A, 0xFB01, 0xFB02,
    0x2021, 0x00B7, 0x201A, 0x201E, 0x2030, 0x00C2, 0x00CA, 0x00C1,
    0x00CB, 0x00C8, 0x00CD, 0x00CE, 0x00CF, 0x00CC, 0x00D3, 0x00D4,
    0xF8FF, 0x00D2, 0x00DA, 0x00DB, 0x00D9, 0x0131, 0x02C6, 0x02DC,
    0x00AF, 0x02D8, 0x02D9, 0x02DA, 0x00B8, 0x02DD, 0x02DB, 0x02C7,
];
