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

//! Compress the default font files using the LZMA algorithm.

use std::env;
use std::fs;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("fonts");
    let fonts_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("src/fonts");

    // Create the out_dir.
    fs::create_dir_all(&out_dir)?;

    for font_file in fs::read_dir(fonts_dir)? {
        let font_file = font_file?.path();
        if font_file
            .extension()
            .map(|e| e.to_str() != Some("ttf"))
            .unwrap_or(true)
        {
            continue;
        }

        // Compress the font file.
        let target_path =
            out_dir.join(Path::new(font_file.file_name().unwrap()).with_extension("lzma"));
        let mut origin_file = BufReader::new(fs::File::open(&font_file)?);
        let mut target_file = BufWriter::new(fs::File::create(&target_path).unwrap());

        lzma_rs::lzma_compress(&mut origin_file, &mut target_file)?;
    }

    Ok(())
}
