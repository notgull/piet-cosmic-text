// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
// This file is a part of `piet-cosmic-text`.
//
// `piet-cosmic-text` is free software: you can redistribute it and/or modify it under the terms of
// either:
//
// * GNU Lesser General Public License as published by the Free Software Foundation, either
// version 3 of the License, or (at your option) any later version.
// * Mozilla Public License as published by the Mozilla Foundation, version 2.
//
// `piet-cosmic-text` is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
// without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
// See the GNU Lesser General Public License or the Mozilla Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License and the Mozilla
// Public License along with `piet-cosmic-text`. If not, see <https://www.gnu.org/licenses/>.

//! Sample 08 from piet.
//! Source: https://github.com/linebender/piet/blob/24acaf6467bff134b7b5c7c0ec70973544020b49/piet/src/samples/picture_8.rs

#[path = "util/display.rs"]
mod display;

use piet::{
    Color, FontFamily, FontStyle, FontWeight, Text as _, TextAlignment, TextAttribute,
    TextLayoutBuilder as _,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let draw = move |text: &mut piet_cosmic_text::Text, _width, _height| {
        static SAMPLE_EN: &str = r#"This essay is an effort to build an ironic political myth faithful to feminism, socialism, and materialism. Perhaps more faithful as blasphemy is faithful, than as reverent worship and identification. Blasphemy has always seemed to require taking things very seriously. I know no better stance to adopt from within the secular-religious, evangelical traditions of United States politics, including the politics of socialist-feminism."#;

        text.new_text_layout(SAMPLE_EN)
            .max_width(200.0)
            .default_attribute(FontFamily::SYSTEM_UI)
            .alignment(TextAlignment::Start)
            //.range_attribute(10..80, TextAttribute::FontSize(8.0))
            .range_attribute(20..120, FontFamily::SERIF)
            .range_attribute(40..60, FontWeight::BOLD)
            .range_attribute(60..140, FontWeight::THIN)
            .range_attribute(90..300, FontFamily::MONOSPACE)
            .range_attribute(120..150, TextAttribute::TextColor(Color::rgb(0.6, 0., 0.)))
            .range_attribute(160..190, TextAttribute::TextColor(Color::rgb(0., 0.6, 0.)))
            .range_attribute(200..240, TextAttribute::TextColor(Color::rgb(0., 0., 0.6)))
            .range_attribute(200.., FontWeight::EXTRA_BLACK)
            //.range_attribute(220.., TextAttribute::FontSize(18.0))
            .range_attribute(240.., FontStyle::Italic)
            .range_attribute(280.., TextAttribute::Underline(true))
            .range_attribute(320.., TextAttribute::Strikethrough(true))
            .build()
            .expect("Failed to build text layout")
    };

    display::run(draw);
    Ok(())
}
