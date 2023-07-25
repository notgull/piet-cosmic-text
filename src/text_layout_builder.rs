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

use crate::attributes::Attributes;
use crate::metadata::Metadata;
use crate::text::{FontSystemAndDefaults, Text};
use crate::text_layout::TextLayout;
use crate::{cvt_color, cvt_family, cvt_style, cvt_weight, warn, FontError, POINTS_PER_INCH};

use cosmic_text as ct;
use ct::{Attrs, Buffer, BufferLine, Metrics};

use piet::{util, Error, TextAlignment, TextAttribute, TextStorage};

use std::fmt;
use std::ops::RangeBounds;

/// The text layout builder used by the [`Text`].
pub struct TextLayoutBuilder {
    /// Handle to the original `Text` object.
    handle: Text,

    /// The string we're laying out.
    string: Box<dyn TextStorage>,

    /// The default text attributes.
    defaults: util::LayoutDefaults,

    /// The width constraint.
    max_width: f64,

    /// Alignment for the text.
    alignment: Option<TextAlignment>,

    /// The range attributes.
    range_attributes: Attributes,

    /// The starting point for the last range.
    ///
    /// Used for error checking.
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

impl TextLayoutBuilder {
    pub(crate) fn new(text: Text, string: impl TextStorage) -> Self {
        Self {
            handle: text,
            string: Box::new(string),
            defaults: util::LayoutDefaults::default(),
            max_width: f64::INFINITY,
            alignment: None,
            last_range_start_pos: 0,
            range_attributes: Attributes::default(),
            error: None,
        }
    }

    fn shaping(&self) -> ct::Shaping {
        // TODO: Use a better strategy to find this!
        ct::Shaping::Advanced
    }
}

impl piet::TextLayoutBuilder for TextLayoutBuilder {
    type Out = TextLayout;

    fn alignment(mut self, alignment: TextAlignment) -> Self {
        self.alignment = Some(alignment);
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

        self.range_attributes.push(range, attribute);

        self
    }

    fn build(self) -> Result<Self::Out, Error> {
        let shaping = self.shaping();
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

        // Get a handle to the font system.
        let mut font_system_guard = handle
            .borrow_font_system()
            .ok_or(Error::BackendError(FontError::AlreadyBorrowed.into()))?;
        let font_system = match font_system_guard.get() {
            Some(font_system) => font_system,
            None => {
                warn!("Still waiting for font system to be loaded, returning error");
                return Err(Error::BackendError(FontError::NotLoaded.into()));
            }
        };

        // Get the font size and line height.
        let font_size = defaults.font_size * handle.dpi() / POINTS_PER_INCH;

        // NOTE: Pango uses a default line height of 0, and piet-cairo doesn't appear to
        // change this.
        let metrics = Metrics::new(font_size as _, font_size as _);

        // Get the default attributes for the layout.
        let default_attrs = {
            let mut metadata = Metadata::new();

            metadata.set_underline(defaults.underline);
            metadata.set_strikethrough(defaults.strikethrough);
            metadata.set_boldness(defaults.weight);

            let mut attrs = Attrs::new()
                .family(cvt_family(&defaults.font))
                .weight(cvt_weight(defaults.weight))
                .style(cvt_style(defaults.style))
                .metadata(metadata.into_raw());

            if defaults.fg_color != util::DEFAULT_TEXT_COLOR {
                attrs = attrs.color(cvt_color(defaults.fg_color));
            }

            font_system.fix_attrs(attrs)
        };

        // Re-use memory from a previous layout.
        let mut buffer_lines = handle.take_buffer();
        let mut offset = 0;

        for line in ct::BidiParagraphs::new(&string) {
            let start = offset;
            let end = start + line.len() + 1;

            // Get the attributes for this line.
            let attrs_list = range_attributes.text_attributes(
                font_system,
                start..end,
                default_attrs.as_attrs(),
            )?;

            let mut line = BufferLine::new(line, attrs_list, shaping);
            line.set_align(self.alignment.map(|a| match a {
                TextAlignment::Start => ct::Align::Left,
                TextAlignment::Center => ct::Align::Center,
                TextAlignment::End => ct::Align::Right,
                TextAlignment::Justified => ct::Align::Justified,
            }));

            buffer_lines.push(line);

            offset = end;
        }

        let buffer = {
            let FontSystemAndDefaults { system, .. } = font_system;
            let mut buffer = Buffer::new(system, metrics);

            buffer.lines = buffer_lines;
            buffer.set_size(system, max_width as f32, f32::INFINITY);
            buffer.set_wrap(system, ct::Wrap::Word);

            // Shape the buffer.
            buffer.shape_until_scroll(system);

            buffer
        };

        drop(font_system_guard);

        Ok(TextLayout::new(handle, buffer, string, font_size as i32))
    }
}
