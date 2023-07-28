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

//! Used to translate `piet` text attributes to `cosmic-text` text attributes.

use crate::metadata::Metadata;
use crate::text::FontSystemAndDefaults;
use crate::{cvt_color, cvt_family, cvt_style, cvt_weight};

use cosmic_text as ct;
use ct::{Attrs, AttrsList, AttrsOwned};

use piet::{util, Error, TextAttribute};

use tinyvec::TinyVec;

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

/// The text attribute ranges.
#[derive(Default)]
pub(crate) struct Attributes {
    /// List of text attributes.
    attributes: Vec<TextAttribute>,

    /// The starts and ends of the range.
    ///
    /// The `usize` in the `RangeEnd` are indices into `attributes`.
    ends: BTreeMap<usize, TinyVec<[RangeEnd; 1]>>,
}

impl fmt::Debug for Attributes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        /// Format a text attribute.
        struct FmtTextAttribute<'a>(&'a Attributes, usize);
        impl fmt::Debug for FmtTextAttribute<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let attr = self.0.attributes.get(self.1).unwrap();
                fmt::Debug::fmt(attr, f)
            }
        }

        /// Format a range end.
        struct WrapFmt<'a, T>(&'a str, T);
        impl<T: fmt::Debug> fmt::Debug for WrapFmt<'_, T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple(self.0).field(&self.1).finish()
            }
        }

        /// Format a list of range ends.
        struct FmtRangeEnds<'a>(&'a Attributes, &'a [RangeEnd]);
        impl fmt::Debug for FmtRangeEnds<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let ends = self.1.iter().map(|end| match end {
                    RangeEnd::Start(index) => WrapFmt("Start", FmtTextAttribute(self.0, *index)),
                    RangeEnd::End(index) => WrapFmt("End", FmtTextAttribute(self.0, *index)),
                });

                f.debug_list().entries(ends).finish()
            }
        }

        /// Format a list of ends.
        struct FmtEnds<'a>(&'a Attributes);
        impl fmt::Debug for FmtEnds<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_map()
                    .entries(
                        self.0
                            .ends
                            .iter()
                            .map(|(&index, ends)| (index, FmtRangeEnds(self.0, ends))),
                    )
                    .finish()
            }
        }

        f.debug_tuple("Attributes").field(&FmtEnds(self)).finish()
    }
}

/// The start or end of a text attribute range.
#[derive(Debug)]
enum RangeEnd {
    /// The start of the range.
    Start(usize),

    /// The end of the range.
    End(usize),
}

/// Not used except for the tinyvec impl.
impl Default for RangeEnd {
    fn default() -> Self {
        Self::Start(0)
    }
}

impl Attributes {
    /// Add a text attribute to the range.
    pub(crate) fn push(&mut self, range: Range<usize>, attr: TextAttribute) {
        // Push the attribute itself.
        let index = self.attributes.len();
        self.attributes.push(attr);

        // Push the range.
        macro_rules! push_index {
            ($pl:ident,$en:ident) => {{
                let end = self.ends.entry(range.$pl).or_default();
                end.push(RangeEnd::$en(index));
            }};
        }

        push_index!(start, Start);
        push_index!(end, End);
    }

    /// Collect text attributes into a list.
    fn collect_attributes<'a>(
        &'a self,
        system: &mut FontSystemAndDefaults,
        mut attrs: Attrs<'a>,
        indices: impl Iterator<Item = usize>,
    ) -> Result<AttrsOwned, Error> {
        macro_rules! with_metadata {
            ($closure:expr) => {{
                // Necessary to tell the compiler about typing info.
                #[inline]
                fn closure_slot(metadata: &mut Metadata, closure: impl FnOnce(&mut Metadata)) {
                    closure(metadata);
                }

                let mut metadata = Metadata::from_raw(attrs.metadata);
                closure_slot(&mut metadata, $closure);
                attrs.metadata = metadata.into_raw();
            }};
        }

        for index in indices {
            let piet_attr = self.attributes.get(index).ok_or_else(|| {
                Error::BackendError(crate::FontError::InvalidAttributeIndex.into())
            })?;
            match piet_attr {
                TextAttribute::FontFamily(family) => {
                    attrs.family = cvt_family(family);
                }
                TextAttribute::FontSize(_size) => {
                    // TODO: cosmic-text does not support variable sized text yet.
                    // https://github.com/pop-os/cosmic-text/issues/64
                    error!("piet-cosmic-text does not support variable size fonts yet");
                }
                TextAttribute::Strikethrough(st) => {
                    with_metadata!(|meta| meta.set_strikethrough(*st));
                }
                TextAttribute::Underline(ul) => {
                    with_metadata!(|meta| meta.set_underline(*ul));
                }
                TextAttribute::Style(style) => {
                    attrs.style = cvt_style(*style);
                }
                TextAttribute::Weight(weight) => {
                    attrs.weight = cvt_weight(*weight);
                    with_metadata!(|meta| meta.set_boldness(*weight));
                }
                TextAttribute::TextColor(color) => {
                    if *color != util::DEFAULT_TEXT_COLOR {
                        attrs.color_opt = Some(cvt_color(*color));
                    } else {
                        attrs.color_opt = None;
                    }
                }
            }
        }

        Ok(system.fix_attrs(attrs))
    }

    /// Iterate over the text attributes.
    pub(crate) fn text_attributes<'a>(
        &'a self,
        system: &mut FontSystemAndDefaults,
        range: Range<usize>,
        defaults: Attrs<'a>,
    ) -> Result<AttrsList, Error> {
        let span = trace_span!("text_attributes", start = range.start, end = range.end);
        let _guard = span.enter();

        let mut last_index = 0;
        let mut result = AttrsList::new(defaults);

        // It may seem like we could use a HashSet here for efficiency, but the order in which the
        // attributes are applied actually matters here. In the future we should investigate more
        // efficient structures for this.
        let mut attr_list = vec![];

        // Get the ranges within the range.
        let mut ranges = self
            .ends
            .iter()
            .filter(|(&index, _)| index < range.end)
            .peekable();

        while let Some((_, ends)) = ranges.next_if(|(&index, _)| index < range.start) {
            // Collect the attributes.
            for end in ends {
                match end {
                    RangeEnd::Start(index) => {
                        // Add the attribute.
                        trace!("adding pre-attribute {}", index);
                        attr_list.push(*index);
                    }
                    RangeEnd::End(index) => {
                        // Remove the attribute.
                        trace!("removing pre-attribute {}", index);
                        attr_list.retain(|&i| i != *index);
                    }
                }
            }
        }

        trace!("end of pre-attributes");

        // Adjust the start index.
        let ranges = ranges.map(|(index, ends)| (index - range.start, ends));

        // Iterate over the ranges.
        for (index, ends) in ranges {
            // Collect the attributes.
            let current_range = last_index..index;
            if !current_range.is_empty() {
                let new_attrs =
                    self.collect_attributes(system, defaults, attr_list.iter().copied())?;
                trace!("adding span {:?}", current_range);
                result.add_span(current_range, new_attrs.as_attrs());
            } else {
                trace!("skipping empty span {:?}", current_range);
            }

            for end in ends {
                match end {
                    RangeEnd::Start(index) => {
                        // Add the attribute.
                        trace!("adding attribute {}", index);
                        attr_list.push(*index);
                    }
                    RangeEnd::End(index) => {
                        // Remove the attribute.
                        trace!("removing attribute {}", index);
                        attr_list.retain(|&i| i != *index);
                    }
                }
            }

            last_index = index;
        }

        // Emit the final span.
        let current_range = last_index..range.end;
        if !current_range.is_empty() {
            let new_attrs = self.collect_attributes(system, defaults, attr_list.into_iter())?;
            trace!("adding final span {:?}", current_range);
            result.add_span(current_range, new_attrs.as_attrs());
        } else {
            trace!("skipping empty final span {:?}", current_range);
        }

        Ok(result)
    }
}
