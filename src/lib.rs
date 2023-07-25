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

//! An implementation of [`piet`]'s text API using [`cosmic-text`].
//!
//! This library implements [`piet`]'s [`Text`] API using primitives from [`cosmic-text`].
//! The intention is for this library to act as a stepping stone to be able to use drawing
//! frameworks that do not natively support text rendering (like OpenGL) by using the
//! [`cosmic-text`] library to render text to a texture, and then using that texture
//! in the drawing framework.
//!
//! This library provides a [`Text`](crate::Text), a [`TextLayoutBuilder`] and a
//! [`TextLayout`]. All of these are intended to be used in the same way as the
//! corresponding types in [`piet`]. However, [`TextLayout`] has a `buffer` method that
//! can be used to get the underlying text.
//!
//! The structures provided by this crate are completely renderer-agnostic. The [`TextLayout`]
//! structure exposes the underlying [`Buffer`] structure, which can be used to render the text.
//!
//! # Embedded Fonts
//!
//! In order to make it easier to use this crate in a cross platform setting, it embeds a handful
//! of fonts into the binary. These fonts are used as a fallback when the system fonts are not
//! available. For instance, on web targets, there are no fonts available by default, so these
//! fonts are used instead. In addition, if attributes fail to match any of the fonts on the
//! system, these fonts are used as a fallback.
//!
//! Without compression, these fonts add around 1.5 megabytes to the final binary. With the
//! `DEFLATE` compression algorithm, which is enabled by default, this is reduced to around
//! 1.1 megabytes. In practice it's actually around 700 kilobytes, as the remaining data is used
//! by the compressions algorithm. [`yazi`] is used to compress the font data; as it is also used
//! by [`swash`], which is often used with [`cosmic-text`], the actual amount of data saved should
//! be closer to the theoretical maximum.
//!
//! To disable font compression, disable the default `compress-fonts` feature. To disable embedding
//! fonts altogether, disable the default `embed-fonts` feature.
//!
//! # Font Initialization
//!
//! The initialization of the [`FontSystem`] can take some time, especially on slower systems with
//! many thousand fonts. In order to prevent font loading from blocking the main windowing thread,
//! [`Text`] has an option to use a background thread to load the fonts. Enabling the `rayon`
//! feature (not enabled by default) will export font loading to the [`rayon`] thread pool.
//! Without this feature, font loading will be done on the current thread.
//!
//! As web targets do not support threads, enabling the `rayon` feature on web targets will lead
//! to compilation errors.
//!
//! Sufficiently complex programs usually already have a system set up to handle blocking tasks.
//! For `async` programs, this is usually [`tokio`]'s [`spawn_blocking`] function or the
//! [`blocking`] thread pool. In these cases you can implement the [`ExportWork`] trait and then
//! pass it to [`Text::with_thread`]. This will allow you to use the same thread pool for both
//! font loading and other blocking tasks.
//!
//! The `is_loaded` method of [`Text`](crate::Text) can be used to check if the font system is
//! fully loaded.
//!
//! # Limitations
//!
//! The text does not support variable font sizes. Attempting to use these will result in emitting
//! an error to the logs, and the text size not actually being changed.
//!
//! [`piet`]: https://docs.rs/piet
//! [`cosmic-text`]: https://docs.rs/cosmic-text
//! [`Buffer`]: https://docs.rs/cosmic-text/latest/cosmic_text/struct.Buffer.html
//! [`Text`]: https://docs.rs/piet/latest/piet/trait.Text.html
//! [`FontSystem`]: https://docs.rs/cosmic-text/latest/cosmic_text/fontdb/struct.FontSystem.html
//! [`yazi`]: https://docs.rs/yazi
//! [`swash`]: https://docs.rs/swash
//! [`rayon`]: https://docs.rs/rayon
//! [`tokio`]: https://docs.rs/tokio
//! [`spawn_blocking`]: https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html
//! [`blocking`]: https://docs.rs/blocking

#![allow(clippy::await_holding_refcell_ref)]
#![forbid(unsafe_code, future_incompatible, rust_2018_idioms)]

// Public dependencies.
pub use cosmic_text;
pub use piet;

use cosmic_text as ct;

use std::fmt;

pub use export_work::{CurrentThread, ExportWork};
pub use lines::{LineProcessor, StyledLine};
pub use metadata::Metadata;
pub use text::Text;
pub use text_layout::TextLayout;
pub use text_layout_builder::TextLayoutBuilder;

#[cfg(feature = "rayon")]
pub use export_work::Rayon;

use text::FontSystemAndDefaults;

const STANDARD_DPI: f64 = 96.0;
const POINTS_PER_INCH: f64 = 72.0;

#[cfg(feature = "tracing")]
use tracing::{error, trace, trace_span, warn, warn_span};

#[cfg(not(feature = "tracing"))]
macro_rules! error {
    ($($tt:tt)*) => {};
}

#[cfg(not(feature = "tracing"))]
macro_rules! warn {
    ($($tt:tt)*) => {};
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace {
    ($($tt:tt)*) => {};
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_span {
    ($($tt:tt)*) => {
        Span
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! warn_span {
    ($($tt:tt)*) => {
        Span
    };
}

#[cfg(not(feature = "tracing"))]
struct Span;

#[cfg(not(feature = "tracing"))]
impl Span {
    fn enter(self) {}
}

mod attributes;
mod channel;
#[cfg(feature = "embed_fonts")]
mod embedded_fonts;
mod export_work;
mod lines;
mod metadata;
mod text;
mod text_layout;
mod text_layout_builder;

/// The error type for this library.
#[derive(Debug)]
pub(crate) enum FontError {
    /// Attempted to mutably borrow the font system twice.
    AlreadyBorrowed,

    /// The font system is not loaded yet.
    NotLoaded,
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyBorrowed => f.write_str("the FontSystem is already mutably borrowed and cannot be accessed"),
            Self::NotLoaded => f.write_str("the FontSystem is not loaded yet, check is_loaded() before accessing or use wait_for_load()"),
        }
    }
}

impl std::error::Error for FontError {}

fn cvt_color(p: piet::Color) -> ct::Color {
    let (r, g, b, a) = p.as_rgba8();
    ct::Color::rgba(r, g, b, a)
}

fn cvt_family(p: &piet::FontFamily) -> ct::Family<'_> {
    macro_rules! generic {
        ($piet:ident => $cosmic:ident) => {
            if p == &piet::FontFamily::$piet {
                return ct::Family::$cosmic;
            }
        };
    }

    if p.is_generic() {
        generic!(SERIF => Serif);
        generic!(SANS_SERIF => SansSerif);
        generic!(MONOSPACE => Monospace);
    }

    ct::Family::Name(p.name())
}

fn cvt_style(p: piet::FontStyle) -> ct::Style {
    use piet::FontStyle;

    match p {
        FontStyle::Italic => ct::Style::Italic,
        FontStyle::Regular => ct::Style::Normal,
    }
}

fn cvt_weight(p: piet::FontWeight) -> ct::Weight {
    ct::Weight(p.to_raw())
}
