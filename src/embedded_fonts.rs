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

//! Fonts that are embedded into the `FontSystem` by default.
//!
//! These fonts act as a backup for when the system fonts are not available. This tends to happen
//! especially on web targets.

use cosmic_text::fontdb::{Source, ID as FontId};
use cosmic_text::FontSystem;

use std::io::Error;
use std::mem;
use std::sync::Arc;

// The raw data emitted by build/embed_fonts.rs.
const FONT_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/font_data/font_data.bin"));

/// Load the embedded font data into the font system.
#[allow(clippy::needless_return)]
pub(super) fn load_embedded_font_data(system: &mut FontSystem) -> Result<Vec<FontId>, Error> {
    #[cfg(not(feature = "compress_fonts"))]
    {
        // Just read straight from the embedded data.
        return read_font_data(system, FONT_DATA);
    }

    #[cfg(feature = "compress_fonts")]
    {
        use std::io::prelude::*;

        // Use `yazi` to decompress the font data.
        let mut decoder = {
            let mut decoder = yazi::Decoder::boxed();
            decoder.set_format(yazi::Format::Raw);
            decoder
        };

        // Write the decoded data into a buffer.
        let mut decoded_data = Vec::new();
        let mut stream = decoder.stream_into_vec(&mut decoded_data);
        stream.write_all(FONT_DATA)?;
        stream.finish().map_err(|_| {
            Error::new(
                std::io::ErrorKind::InvalidData,
                "Failed to decode font data",
            )
        })?;

        return read_font_data(system, &decoded_data);
    }
}

/// Read from font data into the font system.
fn read_font_data(system: &mut FontSystem, mut data: &[u8]) -> Result<Vec<FontId>, Error> {
    let mut all_ids = vec![];

    loop {
        // Get the length of the font.
        let font_len = if data.len() >= mem::size_of::<u64>() {
            let (length, rest) = data.split_at(mem::size_of::<u64>());
            data = rest;
            u64::from_le_bytes(length.try_into().unwrap())
        } else {
            break;
        };

        // Read the font data.
        let (font_data, rest) = data.split_at(font_len.try_into().unwrap());
        data = rest;

        // Insert it into the font system.
        let ids = system
            .db_mut()
            .load_font_source(Source::Binary(Arc::new(font_data.to_vec())));
        assert!(!ids.is_empty());

        for id in ids {
            let font = system.db().face(id);
            if let Some(font) = font {
                for (_name, _) in &font.families {
                    #[cfg(feature = "tracing")]
                    tracing::debug!("Loaded default font: {}", _name);
                }
            }
            all_ids.push(id);
        }
    }

    set_default_fonts(system);

    Ok(all_ids)
}

fn set_default_fonts(fs: &mut FontSystem) {
    fs.db_mut().set_monospace_family("DejaVu Sans Mono");
    fs.db_mut().set_sans_serif_family("DejaVu Sans");
    fs.db_mut().set_serif_family("DejaVu Serif");
}
