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

use piet::{FontFamily, Text as _, TextLayout as _, TextLayoutBuilder as _};
use piet_cosmic_text::Text;

#[test]
fn layout() {
    let mut text = Text::new();
    for _ in 0..2 {
        // Load a layout.
        let test_text: &'static str = "Hello world!";
        let layout = match text
            .new_text_layout(test_text)
            .max_width(100.0)
            .font(FontFamily::SANS_SERIF, 12.0)
            .build()
        {
            Ok(layout) => layout,
            Err(piet::Error::BackendError(_)) => {
                text.wait_for_load_blocking();
                continue;
            }
            Err(e) => panic!("{}", e),
        };

        // Check the layout.
        assert_eq!(
            layout
                .buffer()
                .layout_runs()
                .flat_map(|run| run.glyphs)
                .count(),
            test_text.len()
        );
        assert!(layout.size().width <= 100.0);

        text.wait_for_load_blocking();
    }
}
