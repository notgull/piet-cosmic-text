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

use std::env::var_os;
use std::error::Error;
use std::fs;
use std::io::{self, prelude::*, BufWriter};
use std::path::Path;

type Result = std::result::Result<(), Box<dyn Error>>;

macro_rules! leap {
    ($e:expr, $msg:literal) => {{
        ($e).ok_or_else::<Box<dyn Error>, _>(|| ($msg).into())?
    }};
    ($e:expr) => {{
        ($e).map_err::<Box<dyn Error>, _>(Into::into)?
    }};
}

/// Fonts to embed.
const EMBEDDED_FONTS: &[&str] = &["DejaVuSans", "DejaVuSansMono", "DejaVuSerif"];

/// Embed the font data into the binary.
pub(crate) fn embed_font_data() -> Result {
    let crate_root = leap!(var_os("CARGO_MANIFEST_DIR"), "Failed to get manifest dir");
    let out_dir = leap!(var_os("OUT_DIR"), "Failed to get out dir");
    let font_data_root = Path::new(&crate_root).join("fonts/ttf");
    let font_out_dir = Path::new(&out_dir).join("font_data");

    // Create the output directory.
    fs::create_dir_all(&font_out_dir)?;

    let mut file = BufWriter::new(fs::File::create(font_out_dir.join("font_data.bin"))?);

    // If we aren't compressing the font, just write it all out.
    #[cfg(not(feature = "compress_fonts"))]
    {
        write_font_data(&font_data_root, &mut file)?;
    }

    // If we are compressing the font, write it out using the LZMA2 algorithm.
    #[cfg(feature = "compress_fonts")]
    {
        let mut buf = vec![];
        write_font_data(&font_data_root, &mut buf)?;

        // Compress it and write it to the file.
        lzma_rs::lzma_compress(&mut &*buf, &mut file)?;
    }

    //    panic!("Font data written to {:?}", font_out_dir);

    Ok(())
}

/// Write all of the font data into the provided writer.
fn write_font_data(font_data_root: &Path, mut output: impl Write) -> Result {
    // Poor man's tarball:
    // - First eight bytes are the number of bytes in this font file, in little endian format.
    // - Next N bytes are that font file.
    //
    // Lookup capabilities are not needed in this case.

    for font in EMBEDDED_FONTS {
        let source_path = font_data_root.join(format!("{}.ttf", font));
        let length = fs::metadata(&source_path)?.len();

        // Write the font length.
        let len_bytes = length.to_le_bytes();
        output.write_all(&len_bytes)?;

        // Write the entire data.
        // Since we're reading it all in one shot, no need to use a `BufReader`.
        let mut file = fs::File::open(source_path)?;
        io::copy(&mut file, &mut output)?;
    }

    Ok(())
}
