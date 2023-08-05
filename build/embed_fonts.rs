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

    let file = BufWriter::new(fs::File::create(font_out_dir.join("font_data.bin"))?);

    // If we aren't compressing the font, just write it all out.
    #[cfg(not(feature = "compress_fonts"))]
    {
        write_font_data(&font_data_root, file)?;
    }

    // If we are compressing the font, write it out using the LZMA2 algorithm.
    #[cfg(feature = "compress_fonts")]
    {
        // Compress it and write it to the file.
        //
        // Nota Bene (notgull): Analysis of various compression-based crates for Rust, when it comes
        // to this data.
        //
        // I want a pure-Rust compression crate here, as I'd like as few C libraries in my tree as
        // possible. I've included some crates that use C libraries for comparison.
        //
        // - Uncompressed, the data is around 1.5 MB
        // - With `lzma_rs::lzma2_compress`, it looks to be around 1.5 MB as well. It looks like the
        //   implementation of LZMA2 here doesn't do any actual compression?
        // - With `lzma_rs::lzma_compress` we get down to 1.01 MB.
        // - All of `flate2`'s encoders give us a compression of around 784 KB.
        // - With `zstd`, we get down to 704 KB. This uses a C library, unfortunately.
        // - `rust-lzma` with compression present 6 gets us down to 604 KB.
        // - `xz2` gets us down to a whopping 568 KB.
        // - `lz4` gives us 900 KB.
        // - `snap` gives us 1.1 MB.
        // - `yazi` gets us 784 KB, the same as `flate2`.
        //
        // It looks like the Rust LZMA implementation is still lacking a bit, as it falls far behind
        // the C LZMA and XZ implementations. `xz2` gives us the best compression if we were willing
        // to use C libraries. `flate2` and `yazi` give us the best compression if we want to stick
        // to pure Rust. I prefer `yazi` in this case, as it already exists in the dependency tree
        // for `cosmic-text` thanks to `swash`.
        //
        // For now, this isn't too important. But, in the future, it would be nice to either write
        // a better XZ implementation in Rust or sponsor someone to do that.
        let mut file = file;

        let mut encoder = {
            let mut encoder = yazi::Encoder::boxed();
            encoder.set_format(yazi::Format::Raw);
            encoder.set_level(yazi::CompressionLevel::BestSize);
            encoder
        };

        write_font_data(&font_data_root, encoder.stream(&mut file))?;
    }

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
