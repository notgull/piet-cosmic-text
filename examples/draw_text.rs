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

//! An example for drawing some basic text using `piet-cosmic-text` and `softbuffer`.

#[path = "util/display.rs"]
mod display;

use piet::{FontFamily, Text, TextLayoutBuilder};

fn main() {
    display::run(|text, width, _| {
        text
            .new_text_layout("Line #1\nLine #2\nLine #3\nمرحبا بالعالم\n💀 💀 💀\nThis is an exceptionally long line! foobar foobar foobar foobar")
            .font(FontFamily::SANS_SERIF, 24.0)
            .max_width(width as _)
            .range_attribute(2..10, piet::TextAttribute::Underline(true))
            .build()
            .unwrap()
    })
}
