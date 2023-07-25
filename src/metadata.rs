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

use core::fmt;
use piet::FontWeight;

/// The metadata stored in the font's stylings.
///
/// This should be considered by the renderer in order to render extra decorations.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Metadata(usize);

impl fmt::Debug for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metadata")
            .field("underline", &self.underline())
            .field("strikethrough", &self.strikethrough())
            .field("boldness", &self.boldness())
            .finish()
    }
}

const FONT_WEIGHT_SIZE: usize = 10;
const FONT_WEIGHT_MASK: usize = 0b1111111111;
const UNDERLINE: usize = 1 << FONT_WEIGHT_SIZE;
const STRIKETHROUGH: usize = 1 << (FONT_WEIGHT_SIZE + 1);

impl Metadata {
    /// Create a new, empty metadata.
    pub fn new() -> Self {
        Self(FontWeight::NORMAL.to_raw().into())
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

    /// Set the boldness of the font.
    pub fn set_boldness(&mut self, boldness: FontWeight) {
        self.0 &= !FONT_WEIGHT_MASK;
        self.0 |= usize::from(boldness.to_raw());
    }

    /// Is the "underline" bit set?
    pub fn underline(&self) -> bool {
        self.0 & UNDERLINE != 0
    }

    /// Is the "strikethrough" bit set?
    pub fn strikethrough(&self) -> bool {
        self.0 & STRIKETHROUGH != 0
    }

    /// Get the boldness of the font.
    pub fn boldness(&self) -> FontWeight {
        FontWeight::new((self.0 & FONT_WEIGHT_MASK) as u16)
    }
}
