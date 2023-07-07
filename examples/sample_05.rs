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

//! Adaptation of piet sample #5, from here:
//! https://github.com/linebender/piet/blob/24acaf6467bff134b7b5c7c0ec70973544020b49/piet/src/samples/picture_5.rs

#[path = "util/display.rs"]
mod display;

use piet::{Color, FontFamily, FontStyle, FontWeight, Text, TextAttribute, TextLayoutBuilder};

static TEXT: &str = r#"Philosophers often behave like little children who scribble some marks on a piece of paper at random and then ask the grown-up "What's that?" — It happened like this: the grown-up had drawn pictures for the child several times and said "this is a man," "this is a house," etc. And then the child makes some marks too and asks: what's this then?"#;

const RED: Color = Color::rgb8(255, 0, 0);
const BLUE: Color = Color::rgb8(0, 0, 255);

fn main() {
    display::run(|text, _, _| {
        let courier = text
            .font_family("Courier New")
            .unwrap_or(FontFamily::MONOSPACE);
        text.new_text_layout(TEXT)
            .max_width(200.0)
            .default_attribute(courier)
            .default_attribute(TextAttribute::Underline(true))
            .default_attribute(FontStyle::Italic)
            .default_attribute(TextAttribute::TextColor(RED))
            .default_attribute(FontWeight::BOLD)
            .range_attribute(..200, TextAttribute::TextColor(BLUE))
            .range_attribute(10..100, FontWeight::NORMAL)
            .range_attribute(20..50, TextAttribute::Strikethrough(true))
            .range_attribute(40..300, TextAttribute::Underline(false))
            .range_attribute(60..160, FontStyle::Regular)
            .range_attribute(140..220, FontWeight::NORMAL)
            .range_attribute(240.., FontFamily::SYSTEM_UI)
            .build()
            .unwrap()
    })
}
