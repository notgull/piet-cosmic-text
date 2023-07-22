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
//! # Limitations
//!
//! - New fonts cannot be loaded while a [`TextLayout`] is alive.
//! - The text does not support [`TextAlignment`] or variable font sizes. Attempting to
//!   use these will result in an error.
//!
//! [`piet`]: https://docs.rs/piet
//! [`cosmic-text`]: https://docs.rs/cosmic-text
//! [`TextAlignment`]: https://docs.rs/piet/latest/piet/enum.TextAlignment.html

#![allow(clippy::await_holding_refcell_ref)]
#![forbid(unsafe_code, future_incompatible, rust_2018_idioms)]

// Public dependencies.
pub use cosmic_text;
pub use piet;

mod channel;
mod lines;

use event_listener::Event;

use cosmic_text as ct;

use ct::fontdb::Family;
use ct::{Attrs, AttrsList, Buffer, BufferLine, FontSystem, LayoutRunIter, Metrics};

use piet::kurbo::{Point, Rect, Size};
use piet::{util, Error, FontFamily, FontWeight, TextAlignment, TextAttribute, TextStorage};

use std::cell::{Cell, RefCell, RefMut};
use std::cmp;
use std::collections::BTreeMap;
use std::fmt;
use std::ops::{Deref, DerefMut, Range, RangeBounds};
use std::rc::Rc;
use std::sync::Arc;

const STANDARD_DPI: f64 = 96.0;
const POINTS_PER_INCH: f64 = 72.0;

pub use lines::{LineProcessor, StyledLine};

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

/// Trait for exporting work to another thread.
pub trait ExportWork {
    /// Run this closure on another thread.
    fn run(self, f: impl FnOnce() + Send + 'static);
}

/// Run work on the current thread.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CurrentThread;

impl ExportWork for CurrentThread {
    fn run(self, f: impl FnOnce() + Send + 'static) {
        f()
    }
}

/// Run work on the `rayon` thread pool.
#[cfg(feature = "rayon")]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Rayon;

#[cfg(feature = "rayon")]
impl ExportWork for Rayon {
    fn run(self, f: impl FnOnce() + Send + 'static) {
        rayon_core::spawn(f)
    }
}

/// The text implementation entry point.
///
/// # Limitations
///
/// `load_from_data` should not be called while `TextLayout` objects are alive; otherwise, a panic will
/// occur.
#[derive(Clone)]
pub struct Text(Rc<Inner>);

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Borrowed;
        impl fmt::Debug for Borrowed {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("<borrowed>")
            }
        }

        let mut ds = f.debug_struct("Text");

        // Print out the font database if we can.
        let font_db = self.0.font_db.try_borrow();
        let _ = match &font_db {
            Ok(font_db) => ds.field("font_db", font_db),
            Err(_) => ds.field("font_db", &Borrowed),
        };

        // Finish with the DPI.
        ds.field("dpi", &self.0.dpi.get()).finish()
    }
}

/// Inner shared data.
struct Inner {
    /// Font database.
    font_db: RefCell<DelayedFontSystem>,

    /// Wait for the font database to be free to load.
    font_db_free: Event,

    /// Buffer that holds lines of text.
    ///
    /// These are held here so that they aren't constantly reallocated.
    buffer: Cell<Vec<BufferLine>>,

    /// The current dots-per-inch (DPI) of the rendering surface.
    dpi: Cell<f64>,
}

impl Inner {
    fn borrow_font_system(&self) -> Option<FontSystemGuard<'_>> {
        self.font_db
            .try_borrow_mut()
            .map(|font_db| FontSystemGuard {
                font_db,
                font_db_free: &self.font_db_free,
            })
            .ok()
    }
}

struct FontSystemGuard<'a> {
    /// The font system.
    font_db: RefMut<'a, DelayedFontSystem>,

    /// The event to signal.
    font_db_free: &'a Event,
}

impl Deref for FontSystemGuard<'_> {
    type Target = DelayedFontSystem;

    fn deref(&self) -> &Self::Target {
        &self.font_db
    }
}

impl DerefMut for FontSystemGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.font_db
    }
}

impl Drop for FontSystemGuard<'_> {
    fn drop(&mut self) {
        self.font_db_free.notify(usize::MAX);
    }
}

/// Either a `FontSystem` or a handle that can be resolved to one.
#[allow(clippy::large_enum_variant)] // No need to box the FontSystem since it will be used soon.
enum DelayedFontSystem {
    /// The real font system.
    Real(FontSystem),

    /// We are waiting for a font system to be loaded.
    Waiting(channel::Receiver<FontSystem>),
}

impl fmt::Debug for DelayedFontSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Real(fs) => f
                .debug_struct("FontSystem")
                .field("db", fs.db())
                .field("locale", &fs.locale())
                .finish_non_exhaustive(),
            Self::Waiting(_) => f.write_str("<waiting for availability>"),
        }
    }
}

impl DelayedFontSystem {
    /// Get the font system.
    fn get(&mut self) -> Option<&mut FontSystem> {
        loop {
            match self {
                Self::Real(font_system) => return Some(font_system),
                Self::Waiting(channel) => {
                    // Try to wait on the channel without blocking.
                    *self = Self::Real(channel.try_recv()?);
                }
            }
        }
    }

    /// Wait until the font system is loaded.
    async fn wait(&mut self) -> &mut FontSystem {
        loop {
            match self {
                Self::Real(font_system) => return font_system,
                Self::Waiting(recv) => *self = Self::Real(recv.recv().await),
            }
        }
    }

    /// Wait until the font system is loaded, blocking redux.
    fn wait_blocking(&mut self) -> &mut FontSystem {
        loop {
            match self {
                Self::Real(font_system) => return font_system,
                Self::Waiting(recv) => *self = Self::Real(recv.recv_blocking()),
            }
        }
    }
}

impl Text {
    /// Create a new `Text` renderer.
    pub fn new() -> Self {
        #[cfg(all(feature = "rayon", not(target_arch = "wasm32")))]
        {
            Self::with_thread(Rayon)
        }

        #[cfg(not(all(feature = "rayon", not(target_arch = "wasm32"))))]
        {
            Self::with_thread(CurrentThread)
        }
    }

    /// Create a new `Text` renderer with the given thread to push work to.
    pub fn with_thread(thread: impl ExportWork) -> Self {
        let (send, recv) = channel::channel();

        thread.run(move || {
            let fs = FontSystem::new();
            send.send(fs);
        });

        Self(Rc::new(Inner {
            font_db: RefCell::new(DelayedFontSystem::Waiting(recv)),
            font_db_free: Event::new(),
            buffer: Cell::new(Vec::new()),
            dpi: Cell::new(STANDARD_DPI),
        }))
    }

    /// Create a new `Text` renderer from a `FontSystem`.
    pub fn from_font_system(font_system: FontSystem) -> Self {
        Self(Rc::new(Inner {
            font_db: RefCell::new(DelayedFontSystem::Real(font_system)),
            font_db_free: Event::new(),
            buffer: Cell::new(Vec::new()),
            dpi: Cell::new(STANDARD_DPI),
        }))
    }

    /// Get the current dots-per-inch (DPI) that this text renderer will use.
    pub fn dpi(&self) -> f64 {
        self.0.dpi.get()
    }

    /// Set the current dots-per-inch (DPI) that this renderer will use.
    ///
    /// Returns the old DPI.
    pub fn set_dpi(&self, dpi: f64) -> f64 {
        self.0.dpi.replace(dpi)
    }

    /// Tell if the font system is loaded.
    pub fn is_loaded(&self) -> bool {
        self.0
            .borrow_font_system()
            .map_or(false, |mut font_db| font_db.get().is_some())
    }

    /// Wait for the font system to be loaded.
    pub async fn wait_for_load(&self) {
        loop {
            if let Ok(mut guard) = self.0.font_db.try_borrow_mut() {
                guard.wait().await;
                return;
            }

            // Create an event listener.
            let listener = self.0.font_db_free.listen();

            if let Ok(mut guard) = self.0.font_db.try_borrow_mut() {
                guard.wait().await;
                return;
            }

            // Wait for the event to be signaled.
            listener.await;
        }
    }

    /// Wait for the font system to be loaded, blocking redux.
    pub fn wait_for_load_blocking(&self) {
        loop {
            if let Ok(mut guard) = self.0.font_db.try_borrow_mut() {
                guard.wait_blocking();
                return;
            }

            // Create an event listener.
            let listener = self.0.font_db_free.listen();

            if let Ok(mut guard) = self.0.font_db.try_borrow_mut() {
                guard.wait_blocking();
                return;
            }

            // Wait for the event to be signaled.
            listener.wait();
        }
    }

    /// Run a closure with mutable access to the underlying `FontSystem`.
    ///
    /// # Notes
    ///
    /// Loading new fonts while this function is in use will result in an error.
    pub fn with_font_system_mut<R>(&self, f: impl FnOnce(&mut FontSystem) -> R) -> Option<R> {
        let mut font_db = self.0.borrow_font_system()?;
        font_db.get().map(f)
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}

impl piet::Text for Text {
    type TextLayout = TextLayout;
    type TextLayoutBuilder = TextLayoutBuilder;

    fn font_family(&mut self, family_name: &str) -> Option<FontFamily> {
        let mut db_guard = self.0.borrow_font_system()?;
        let db = db_guard.get()?;

        // Look to see where it's used.
        for (name, piet_name) in [
            (Family::Serif, FontFamily::SERIF),
            (Family::SansSerif, FontFamily::SANS_SERIF),
            (Family::Monospace, FontFamily::MONOSPACE),
        ] {
            let name = db.db().family_name(&name);
            if name == family_name {
                return Some(piet_name);
            }
        }

        // Get the font family.
        let family = Family::Name(family_name);
        let name = db.db().family_name(&family);

        // Look for the font with that name.
        let font = db
            .db()
            .faces()
            .flat_map(|face| &face.families)
            .find(|(face, _)| *face == name)
            .map(|(face, _)| FontFamily::new_unchecked(face.clone()));

        font
    }

    fn load_font(&mut self, data: &[u8]) -> Result<FontFamily, Error> {
        let span = warn_span!("load_font", data_len = data.len());
        let _enter = span.enter();

        let mut db_guard = self
            .0
            .borrow_font_system()
            .ok_or_else(|| Error::BackendError(FontError::AlreadyBorrowed.into()))?;
        let db = db_guard
            .get()
            .ok_or_else(|| Error::BackendError(FontError::NotLoaded.into()))?;

        // Insert the data source into the underlying font database.
        let id = {
            let ids = db
                .db_mut()
                .load_font_source(ct::fontdb::Source::Binary(Arc::new(data.to_vec())));

            // For simplicity, just take the first ID if this is a font collection.
            match ids.len() {
                0 => return Err(Error::FontLoadingFailed),
                1 => ids[0],
                _len => {
                    warn!("received font collection of length {_len}, only selecting first font");
                    ids[0]
                }
            }
        };

        // Get the font back.
        let font = db.db().face(id).ok_or_else(|| Error::FontLoadingFailed)?;
        Ok(FontFamily::new_unchecked(font.families[0].0.as_str()))
    }

    fn new_text_layout(&mut self, text: impl TextStorage) -> Self::TextLayoutBuilder {
        let text = Box::new(text);
        let defaults = util::LayoutDefaults::default();

        TextLayoutBuilder {
            handle: self.clone(),
            string: text,
            defaults,
            range_attributes: Attributes::default(),
            last_range_start_pos: 0,
            max_width: f64::INFINITY,
            error: None,
            alignment: None,
        }
    }
}

/// The text layout builder used by the [`Text`].
pub struct TextLayoutBuilder {
    /// Handle to the original `Text` object.
    handle: Text,

    /// The string we're laying out.
    string: Box<dyn TextStorage>,

    /// The default text attributes.
    defaults: util::LayoutDefaults,

    /// The width constraint.
    max_width: f64,

    /// Alignment for the text.
    alignment: Option<TextAlignment>,

    /// The range attributes.
    range_attributes: Attributes,

    /// The starting point for the last range.
    ///
    /// Used for error checking.
    last_range_start_pos: usize,

    /// The last error that occurred.
    error: Option<Error>,
}

impl fmt::Debug for TextLayoutBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextLayoutBuilder")
            .field("string", &self.string.as_str())
            .field("max_width", &self.max_width)
            .field("range_attributes", &self.range_attributes)
            .finish_non_exhaustive()
    }
}

impl TextLayoutBuilder {
    fn shaping(&self) -> ct::Shaping {
        // TODO: Use a better strategy to find this!
        ct::Shaping::Advanced
    }
}

impl piet::TextLayoutBuilder for TextLayoutBuilder {
    type Out = TextLayout;

    fn alignment(mut self, alignment: TextAlignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    fn max_width(mut self, width: f64) -> Self {
        self.max_width = width;
        self
    }

    fn default_attribute(mut self, attribute: impl Into<TextAttribute>) -> Self {
        self.defaults.set(attribute);
        self
    }

    fn range_attribute(
        mut self,
        range: impl RangeBounds<usize>,
        attribute: impl Into<TextAttribute>,
    ) -> Self {
        let range = util::resolve_range(range, self.string.len());
        let attribute = attribute.into();

        debug_assert!(
            range.start >= self.last_range_start_pos,
            "attributes must be added in non-decreasing start order"
        );
        self.last_range_start_pos = range.start;

        self.range_attributes.push(range, attribute);

        self
    }

    fn build(self) -> Result<Self::Out, Error> {
        let shaping = self.shaping();
        let Self {
            handle,
            string,
            defaults,
            max_width,
            range_attributes,
            error,
            ..
        } = self;

        // If an error occurred, return it.
        if let Some(error) = error {
            return Err(error);
        }

        // Get the font size and line height.
        let font_size = defaults.font_size * handle.dpi() / POINTS_PER_INCH;

        // NOTE: Pango uses a default line height of 0, and piet-cairo doesn't appear to
        // change this.
        let metrics = Metrics::new(font_size as _, font_size as _);

        // Get the default attributes for the layout.
        let default_attrs = {
            let mut metadata = Metadata::new();

            metadata.set_underline(defaults.underline);
            metadata.set_strikethrough(defaults.strikethrough);
            metadata.set_boldness(defaults.weight);

            let mut attrs = Attrs::new()
                .family(cvt_family(&defaults.font))
                .weight(cvt_weight(defaults.weight))
                .style(cvt_style(defaults.style))
                .metadata(metadata.into_raw());

            if defaults.fg_color != util::DEFAULT_TEXT_COLOR {
                attrs = attrs.color(cvt_color(defaults.fg_color));
            }

            attrs
        };

        // Re-use memory from a previous layout.
        let mut buffer_lines = handle.0.buffer.take();
        let mut offset = 0;

        for line in ct::BidiParagraphs::new(&string) {
            let start = offset;
            let end = start + line.len() + 1;

            // Get the attributes for this line.
            let attrs_list = range_attributes.text_attributes(start..end, default_attrs)?;

            let mut line = BufferLine::new(line, attrs_list, shaping);
            line.set_align(self.alignment.map(|a| match a {
                TextAlignment::Start => ct::Align::Left,
                TextAlignment::Center => ct::Align::Center,
                TextAlignment::End => ct::Align::Right,
                TextAlignment::Justified => ct::Align::Justified,
            }));

            buffer_lines.push(line);

            offset = end;
        }

        let buffer = {
            let mut font_system = handle
                .0
                .borrow_font_system()
                .ok_or(Error::FontLoadingFailed)?;
            let font_system = match font_system.get() {
                Some(font_system) => font_system,
                None => {
                    warn!("Still waiting for font system to be loaded, returning error");
                    return Err(Error::BackendError(FontError::NotLoaded.into()));
                }
            };

            let mut buffer = Buffer::new(font_system, metrics);

            buffer.lines = buffer_lines;
            buffer.set_size(font_system, max_width as f32, f32::INFINITY);
            buffer.set_wrap(font_system, ct::Wrap::Word);

            // Shape the buffer.
            buffer.shape_until_scroll(font_system);

            buffer
        };

        // Figure out the metrics.
        let run_metrics = buffer
            .layout_runs()
            .map(|run| RunMetrics::new(run, font_size))
            .map(|RunMetrics { line_metric }| line_metric)
            .collect();

        Ok(TextLayout {
            text_buffer: Rc::new(BufferWrapper {
                string,
                glyph_size: font_size as i32,
                buffer: Some(buffer),
                run_metrics,
                handle,
            }),
        })
    }
}

/// A text layout.
#[derive(Clone)]
pub struct TextLayout {
    /// The text buffer.
    text_buffer: Rc<BufferWrapper>,
}

impl fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextLayout")
            .field("string", &self.text_buffer.string.as_str())
            .field("glyph_size", &self.text_buffer.glyph_size)
            .finish_non_exhaustive()
    }
}

struct BufferWrapper {
    /// The original string.
    string: Box<dyn TextStorage>,

    /// The size of the glyph in pixels.
    glyph_size: i32,

    /// The original buffer.
    buffer: Option<Buffer>,

    /// Run metrics.
    run_metrics: Vec<piet::LineMetric>,

    /// The text handle.
    handle: Text,
}

impl BufferWrapper {
    fn buffer(&self) -> &Buffer {
        self.buffer.as_ref().unwrap()
    }
}

impl Drop for BufferWrapper {
    fn drop(&mut self) {
        let mut buffer = self.buffer.take().unwrap();
        buffer.lines.clear();
        let old_lines = self.handle.0.buffer.take();

        // Use whichever buffer has the most lines.
        if old_lines.capacity() > buffer.lines.capacity() {
            self.handle.0.buffer.set(old_lines);
        } else {
            self.handle.0.buffer.set(buffer.lines);
        }
    }
}

impl TextLayout {
    /// Get a reference to the inner `Buffer`.
    pub fn buffer(&self) -> &Buffer {
        self.text_buffer.buffer()
    }

    /// Get an iterator over the layout runs.
    pub fn layout_runs(&self) -> LayoutRunIter<'_> {
        self.buffer().layout_runs()
    }
}

impl piet::TextLayout for TextLayout {
    fn size(&self) -> Size {
        self.layout_runs()
            .fold(Size::new(0.0, 0.0), |mut size, run| {
                let max_glyph_size = run
                    .glyphs
                    .iter()
                    .map(|glyph| glyph.font_size as i32)
                    .max()
                    .unwrap_or(self.text_buffer.glyph_size);

                let new_width = run.line_w as f64;
                if new_width > size.width {
                    size.width = new_width;
                }

                let new_height = (run.line_y as i32 + max_glyph_size) as f64;
                if new_height > size.height {
                    size.height = new_height;
                }

                size
            })
    }

    fn trailing_whitespace_width(&self) -> f64 {
        // TODO: This doesn't matter I think.
        self.size().width
    }

    fn image_bounds(&self) -> Rect {
        // TODO: Make this more exact.
        Rect::from_origin_size(Point::ZERO, self.size())
    }

    fn text(&self) -> &str {
        &self.text_buffer.string
    }

    fn line_text(&self, line_number: usize) -> Option<&str> {
        let run = self.buffer().layout_runs().nth(line_number)?;

        if run.glyphs.is_empty() {
            return None;
        }

        let start = run.glyphs[0].start;
        let end = run.glyphs.last().unwrap().end;

        Some(&self.text_buffer.string[start..end])
    }

    fn line_metric(&self, line_number: usize) -> Option<piet::LineMetric> {
        self.text_buffer.run_metrics.get(line_number).cloned()
    }

    fn line_count(&self) -> usize {
        self.buffer().layout_runs().count()
    }

    fn hit_test_point(&self, point: Point) -> piet::HitTestPoint {
        let mut htp = piet::HitTestPoint::default();
        let (x, y) = point.into();

        if let Some(cursor) = self.buffer().hit(x as f32, y as f32) {
            htp.idx = cursor.index;
            htp.is_inside = true;
            return htp;
        }

        // TODO
        htp
    }

    fn hit_test_text_position(&self, idx: usize) -> piet::HitTestPosition {
        // Iterator over glyphs and their assorted lines.
        let mut lines_and_glyphs = self.layout_runs().enumerate().flat_map(|(line, run)| {
            run.glyphs.iter().map(move |glyph| {
                (
                    line,
                    {
                        // Get the point.
                        let physical = glyph.physical((0.0, 0.0), 1.0);
                        let x = physical.x as f64;
                        let y = run.line_y as f64
                            + physical.y as f64
                            + self.text_buffer.glyph_size as f64;

                        Point::new(x, y)
                    },
                    glyph.start..glyph.end,
                )
            })
        });

        let (line, point, _) = match lines_and_glyphs.find(|(_, _, range)| range.contains(&idx)) {
            Some(x) => x,
            None => return piet::HitTestPosition::default(),
        };

        let mut htp = piet::HitTestPosition::default();
        htp.point = point;
        htp.line = line;
        htp
    }
}

/// The text attribute ranges.
#[derive(Default)]
struct Attributes {
    /// List of text attributes.
    attributes: Vec<TextAttribute>,

    /// The starts and ends of the range.
    ///
    /// The `usize` in the `RangeEnd` are indices into `attributes`.
    ends: BTreeMap<usize, Vec<RangeEnd>>,
}

impl fmt::Debug for Attributes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        /// Format a text attribute.
        struct FmtTextAttribute<'a>(&'a Attributes, usize);
        impl fmt::Debug for FmtTextAttribute<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let attr = self.0.attributes.get(self.1).unwrap();
                fmt::Debug::fmt(attr, f)
            }
        }

        /// Format a range end.
        struct WrapFmt<'a, T>(&'a str, T);
        impl<T: fmt::Debug> fmt::Debug for WrapFmt<'_, T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple(self.0).field(&self.1).finish()
            }
        }

        /// Format a list of range ends.
        struct FmtRangeEnds<'a>(&'a Attributes, &'a [RangeEnd]);
        impl fmt::Debug for FmtRangeEnds<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let ends = self.1.iter().map(|end| match end {
                    RangeEnd::Start(index) => WrapFmt("Start", FmtTextAttribute(self.0, *index)),
                    RangeEnd::End(index) => WrapFmt("End", FmtTextAttribute(self.0, *index)),
                });

                f.debug_list().entries(ends).finish()
            }
        }

        /// Format a list of ends.
        struct FmtEnds<'a>(&'a Attributes);
        impl fmt::Debug for FmtEnds<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_map()
                    .entries(
                        self.0
                            .ends
                            .iter()
                            .map(|(&index, ends)| (index, FmtRangeEnds(self.0, ends))),
                    )
                    .finish()
            }
        }

        f.debug_tuple("Attributes").field(&FmtEnds(self)).finish()
    }
}

/// The start or end of a text attribute range.
#[derive(Debug)]
enum RangeEnd {
    /// The start of the range.
    Start(usize),

    /// The end of the range.
    End(usize),
}

impl Attributes {
    /// Add a text attribute to the range.
    fn push(&mut self, range: Range<usize>, attr: TextAttribute) {
        // Push the attribute itself.
        let index = self.attributes.len();
        self.attributes.push(attr);

        // Push the range.
        macro_rules! push_index {
            ($pl:ident,$en:ident) => {{
                let end = self.ends.entry(range.$pl).or_default();
                end.push(RangeEnd::$en(index));
            }};
        }

        push_index!(start, Start);
        push_index!(end, End);
    }

    /// Collect text attributes into a list.
    fn collect_attributes<'a>(
        &'a self,
        mut attrs: Attrs<'a>,
        indices: impl Iterator<Item = usize>,
    ) -> Result<Attrs<'a>, Error> {
        macro_rules! with_metadata {
            ($closure:expr) => {{
                // Necessary to tell the compiler about typing info.
                #[inline]
                fn closure_slot(metadata: &mut Metadata, closure: impl FnOnce(&mut Metadata)) {
                    closure(metadata);
                }

                let mut metadata = Metadata::from_raw(attrs.metadata);
                closure_slot(&mut metadata, $closure);
                attrs.metadata = metadata.into_raw();
            }};
        }

        for index in indices {
            let piet_attr = self
                .attributes
                .get(index)
                .ok_or_else(|| Error::BackendError("invalid attribute index".into()))?;
            match piet_attr {
                TextAttribute::FontFamily(family) => {
                    attrs.family = cvt_family(family);
                }
                TextAttribute::FontSize(_) => {
                    // TODO: cosmic-text does not support variable sized text yet.
                    // https://github.com/pop-os/cosmic-text/issues/64
                    error!("piet-cosmic-text does not support variable size fonts yet");
                }
                TextAttribute::Strikethrough(st) => {
                    with_metadata!(|meta| meta.set_strikethrough(*st));
                }
                TextAttribute::Underline(ul) => {
                    with_metadata!(|meta| meta.set_underline(*ul));
                }
                TextAttribute::Style(style) => {
                    attrs.style = cvt_style(*style);
                }
                TextAttribute::Weight(weight) => {
                    attrs.weight = cvt_weight(*weight);
                    with_metadata!(|meta| meta.set_boldness(*weight));
                }
                TextAttribute::TextColor(color) => {
                    if *color != util::DEFAULT_TEXT_COLOR {
                        attrs.color_opt = Some(cvt_color(*color));
                    } else {
                        attrs.color_opt = None;
                    }
                }
            }
        }

        Ok(attrs)
    }

    /// Iterate over the text attributes.
    fn text_attributes<'a>(
        &'a self,
        range: Range<usize>,
        defaults: Attrs<'a>,
    ) -> Result<AttrsList, Error> {
        let span = trace_span!("text_attributes", start = range.start, end = range.end);
        let _guard = span.enter();

        let mut last_index = 0;
        let mut attr_list = vec![];
        let mut result = AttrsList::new(defaults);

        // Get the ranges within the range.
        let mut ranges = self
            .ends
            .iter()
            .filter(|(&index, _)| index < range.end)
            .peekable();

        while let Some((_, ends)) = ranges.next_if(|(&index, _)| index < range.start) {
            // Collect the attributes.
            for end in ends {
                match end {
                    RangeEnd::Start(index) => {
                        // Add the attribute.
                        trace!("adding pre-attribute {}", index);
                        attr_list.push(*index);
                    }
                    RangeEnd::End(index) => {
                        // Remove the attribute.
                        trace!("removing pre-attribute {}", index);
                        attr_list.retain(|&i| i != *index);
                    }
                }
            }
        }

        trace!("end of pre-attributes");

        // Adjust the start index.
        let ranges = ranges.map(|(index, ends)| (index - range.start, ends));

        // Iterate over the ranges.
        for (index, ends) in ranges {
            // Collect the attributes.
            let current_range = last_index..index;
            if !current_range.is_empty() {
                let new_attrs = self.collect_attributes(defaults, attr_list.iter().copied())?;
                trace!("adding span {:?}", current_range);
                result.add_span(current_range, new_attrs);
            } else {
                trace!("skipping empty span {:?}", current_range);
            }

            for end in ends {
                match end {
                    RangeEnd::Start(index) => {
                        // Add the attribute.
                        trace!("adding attribute {}", index);
                        attr_list.push(*index);
                    }
                    RangeEnd::End(index) => {
                        // Remove the attribute.
                        trace!("removing attribute {}", index);
                        attr_list.retain(|&i| i != *index);
                    }
                }
            }

            last_index = index;
        }

        // Emit the final span.
        let current_range = last_index..range.end;
        if !current_range.is_empty() {
            let new_attrs = self.collect_attributes(defaults, attr_list.into_iter())?;
            trace!("adding final span {:?}", current_range);
            result.add_span(current_range, new_attrs);
        } else {
            trace!("skipping empty final span {:?}", current_range);
        }

        Ok(result)
    }
}

/// Line metrics associated with a layout run.
struct RunMetrics {
    /// The `piet` line metrics.
    line_metric: piet::LineMetric,
}

impl RunMetrics {
    fn new(run: ct::LayoutRun<'_>, glyph_size: f64) -> RunMetrics {
        let (start_offset, end_offset) = run.glyphs.iter().fold((0, 0), |(start, end), glyph| {
            (cmp::min(start, glyph.start), cmp::max(end, glyph.end))
        });

        let y_offset = run.line_top.into();
        let baseline = run.line_y as f64 - run.line_top as f64;

        RunMetrics {
            line_metric: piet::LineMetric {
                start_offset,
                end_offset,
                trailing_whitespace: 0, // TODO
                y_offset,
                height: glyph_size as _,
                baseline,
            },
        }
    }
}

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
