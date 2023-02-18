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

use cosmic_text::fontdb::{Database, Family};
use cosmic_text::{Attrs, AttrsList, Buffer, BufferLine, FontSystem, LayoutRunIter, Metrics};

use piet::kurbo::{Point, Rect, Size};
use piet::{util, LineMetric};
use piet::{Error, FontFamily, TextAlignment, TextAttribute, TextStorage};

use std::cell::{Cell, Ref, RefCell};
use std::fmt;
use std::mem;
use std::ops::{Range, RangeBounds};
use std::rc::Rc;

/// The metadata stored in the font's stylings.
///
/// This should be considered by the renderer in order to render extra decorations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Metadata(usize);

const UNDERLINE: usize = 1 << 0;
const STRIKETHROUGH: usize = 1 << 1;

impl Metadata {
    /// Create a new, empty metadata.
    pub fn new() -> Self {
        Self(0)
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

    /// Is the "underline" bit set?
    pub fn underline(&self) -> bool {
        self.0 & UNDERLINE != 0
    }

    /// Is the "strikethrough" bit set?
    pub fn strikethrough(&self) -> bool {
        self.0 & STRIKETHROUGH != 0
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

struct Inner {
    /// Font database.
    font_db: RefCell<FontDatabase>,

    /// Buffer that holds lines of text.
    ///
    /// These are held here so that they aren't constantly reallocated.
    buffer: Cell<Vec<BufferLine>>,
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Text { .. }")
    }
}

impl Text {
    /// Create a new `Text` renderer.
    pub fn new() -> Self {
        Self::from_font_system(FontSystem::new())
    }

    /// Create a new `Text` renderer from a `FontSystem`.
    pub fn from_font_system(font_system: FontSystem) -> Self {
        Self(Rc::new(Inner {
            font_db: RefCell::new(FontDatabase::Cosmic(font_system)),
            buffer: Cell::new(Vec::new()),
        }))
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
        self.0
            .font_db
            .try_borrow()
            .ok()
            .and_then(|font_db| font_db.font_family_by_name(family_name))
    }

    fn load_font(&mut self, data: &[u8]) -> Result<FontFamily, Error> {
        self.0
            .font_db
            .try_borrow_mut()
            .map_err(|_| {
                Error::BackendError(
                    "tried to load font while TextLayoutBuilder/TextLayout is alive".into(),
                )
            })
            .and_then(|mut font_db| font_db.load_font(data))
    }

    fn new_text_layout(&mut self, text: impl TextStorage) -> Self::TextLayoutBuilder {
        // Force the font database to enter FontSystem mode.
        if !matches!(&*self.0.font_db.borrow(), FontDatabase::Cosmic(_)) {
            self.0.font_db.borrow_mut().font_system();
        }

        let text = Rc::new(text);

        TextLayoutBuilder {
            handle: self.clone(),
            string: text,
            defaults: util::LayoutDefaults::default(),
            range_attributes: vec![],
            last_range_start_pos: 0,
            max_width: f64::INFINITY,
            error: None,
        }
    }
}

/// The text layout builder used by the [`RenderContext`].
pub struct TextLayoutBuilder {
    /// Handle to the original `Text` object.
    handle: Text,

    /// The string we're laying out.
    string: Rc<dyn TextStorage>,

    /// The default text attributes.
    defaults: util::LayoutDefaults,

    /// The width constraint.
    max_width: f64,

    /// The range attributes.
    range_attributes: Vec<(Range<usize>, TextAttribute)>,
    last_range_start_pos: usize,

    /// The last error that occurred.
    error: Option<Error>,
}

impl piet::TextLayoutBuilder for TextLayoutBuilder {
    type Out = TextLayout;

    fn alignment(mut self, alignment: TextAlignment) -> Self {
        // TODO: Support alignment.
        if !matches!(alignment, TextAlignment::Start) {
            self.error = Some(Error::NotSupported);
        }
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

        self.range_attributes.push((range, attribute));

        self
    }

    fn build(self) -> Result<Self::Out, Error> {
        let Self {
            handle,
            string,
            defaults,
            max_width,

            error,
            ..
        } = self;

        // If an error occurred, return it.
        if let Some(error) = error {
            return Err(error);
        }

        // Get the font size and line height.
        let font_size = points_to_pixels(defaults.font_size);

        // NOTE: Pango uses a default line height of 0, and piet-cairo doesn't appear to
        // change this.
        let metrics = Metrics::new(font_size as _, 0);

        // Get the default attributes for the layout.
        let default_attrs = {
            let mut metadata = Metadata::new();

            if defaults.underline {
                metadata.set_underline(true);
            }

            if defaults.strikethrough {
                metadata.set_strikethrough(true);
            }

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

        for line in string.lines() {
            let start = offset;
            let end = start + line.len() + 1;

            // Get the attributes for this line.
            let attrs_list = AttrsList::new(default_attrs);

            // TODO: normalize everything here

            buffer_lines.push(BufferLine::new(line, attrs_list));

            offset = end;
        }

        // Build the self-referencing structure.
        let inner = SelfRefBufferInnerTryBuilder::<_, _, Error> {
            text: handle,
            font_db_builder: |text| Ok(text.0.font_db.borrow()),
            buffer_builder: move |font_db| {
                let font_system = match &**font_db {
                    FontDatabase::Cosmic(font_system) => font_system,
                    FontDatabase::FontDb { .. } => {
                        return Err(Error::BackendError(
                            "font was added while TextLayoutBuilder is alive".into(),
                        ));
                    }
                    _ => unreachable!(),
                };

                let mut buffer = Buffer::new(font_system, metrics);
                buffer.lines = buffer_lines;
                buffer.set_size(max_width as i32, i32::MAX);
                buffer.set_wrap(cosmic_text::Wrap::Word);

                // Shape the buffer.
                buffer.shape_until_scroll();

                Ok(buffer)
            },
        }
        .try_build()?;

        Ok(TextLayout {
            string,
            glyph_size: font_size as i32,
            buffer: Rc::new(SelfRefBuffer(inner)),
        })
    }
}

/// A text layout.
#[derive(Clone)]
pub struct TextLayout {
    /// The original string.
    string: Rc<dyn TextStorage>,

    /// The size of the glyph in pixels.
    glyph_size: i32,

    /// A wrapper around a buffer but with the lifetime requirement elided.
    buffer: Rc<SelfRefBuffer>,
}

/// A wrapper around a self-referencing buffer that we can impl Drop on.
struct SelfRefBuffer(SelfRefBufferInner);

#[ouroboros::self_referencing]
struct SelfRefBufferInner {
    /// Handle to the `Text` renderer.
    text: Text,

    /// Reference to the font database.
    #[borrows(text)]
    #[covariant]
    font_db: Ref<'this, FontDatabase>,

    /// The buffer that holds the rendered text.
    #[borrows(font_db)]
    #[covariant]
    buffer: Buffer<'this>,
}

impl Drop for SelfRefBuffer {
    fn drop(&mut self) {
        let text = self.0.borrow_text().clone();
        self.0.with_buffer_mut(|buf| {
            let mut old_cache = text.0.buffer.take();
            let mut new_cache = mem::take(&mut buf.lines);
            new_cache.clear();

            // If the capacity of the new cache is larger than the old cache, swap them.
            if new_cache.capacity() > old_cache.capacity() {
                mem::swap(&mut new_cache, &mut old_cache);
            }

            text.0.buffer.set(old_cache);
        })
    }
}

impl TextLayout {
    /// Get a reference to the inner `Buffer`.
    pub fn buffer(&self) -> &Buffer<'_> {
        self.buffer.0.borrow_buffer()
    }

    /// Get an iterator over the layout runs.
    pub fn layout_runs(&self) -> LayoutRunIter<'_, '_> {
        self.buffer().layout_runs()
    }

    /// Get an iterator over the line metrics.
    fn line_metrics(&self) -> impl Iterator<Item = LineMetric> + '_ {
        self.layout_runs().map(|_layout| todo!())
    }
}

impl piet::TextLayout for TextLayout {
    fn size(&self) -> Size {
        self.layout_runs()
            .fold(Size::new(0.0, 0.0), |mut size, run| {
                let max_glyph_size = run
                    .glyphs
                    .iter()
                    .map(|glyph| glyph.cache_key.font_size)
                    .max()
                    .unwrap_or(self.glyph_size);

                let new_width = run.line_w as f64;
                if new_width > size.width {
                    size.width = new_width;
                }

                let new_height = (run.line_y + max_glyph_size) as f64;
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
        &self.string
    }

    fn line_text(&self, line_number: usize) -> Option<&str> {
        let run = self.buffer().layout_runs().nth(line_number)?;

        if run.glyphs.is_empty() {
            return None;
        }

        let start = run.glyphs[0].start;
        let end = run.glyphs.last().unwrap().end;

        Some(&self.string[start..end])
    }

    fn line_metric(&self, line_number: usize) -> Option<piet::LineMetric> {
        self.line_metrics().nth(line_number)
    }

    fn line_count(&self) -> usize {
        self.buffer().layout_runs().count()
    }

    fn hit_test_point(&self, point: Point) -> piet::HitTestPoint {
        let mut htp = piet::HitTestPoint::default();
        let (x, y) = point.into();

        if let Some(cursor) = self.buffer().hit(x as i32, y as i32) {
            htp.idx = cursor.index;
            htp.is_inside = true;
            return htp;
        }

        todo!("Calculate the closest index to the point.")
    }

    fn hit_test_text_position(&self, idx: usize) -> piet::HitTestPosition {
        // Iterator over glyphs and their assorted lines.
        let mut lines_and_glyphs = self.layout_runs().enumerate().flat_map(|(line, run)| {
            run.glyphs.iter().map(move |glyph| {
                (
                    line,
                    {
                        // Get the point.
                        let x = glyph.x_int as f64;
                        let y = run.line_y as f64 + glyph.y_int as f64 + self.glyph_size as f64;

                        Point::new(x, y)
                    },
                    glyph.start..glyph.end,
                )
            })
        });

        let (line, point, _) = lines_and_glyphs
            .find(|(_, _, range)| range.contains(&idx))
            .expect("Index out of bounds.");

        let mut htp = piet::HitTestPosition::default();
        htp.point = point;
        htp.line = line;
        htp
    }
}

enum FontDatabase {
    /// The raw `fontdb` font database.
    ///
    /// This is used for adding new fonts to the system.
    FontDb { _locale: String, db: Database },

    /// The `cosmic-text` `FontSystem` structure.
    ///
    /// This is used to render text. It is not cheap to construct, so it should only be
    /// constructed/destructed when we have to add new fonts.
    Cosmic(FontSystem),

    /// Empty hole.
    Empty,
}

impl FontDatabase {
    /// Get a font family by its name.
    fn font_family_by_name(&self, name: &str) -> Option<FontFamily> {
        let db = self.database();

        // Get the font family.
        let family = Family::Name(name);
        let name = db.family_name(&family);

        // Look for the font with that name.
        db.faces()
            .iter()
            .find(|face| face.family == name)
            .map(|face| FontFamily::new_unchecked(face.family.clone()))
    }

    /// Load a font by its raw bytes.
    fn load_font(&mut self, bytes: &[u8]) -> Result<FontFamily, Error> {
        let font_name = font_name(bytes)?;

        // Fast path: try to load the font by its name.
        if let Some(family) = self.font_family_by_name(&font_name) {
            return Ok(family);
        }

        // Slow path: insert the font into the database and then try to load it by its name.
        {
            let db = self.database_mut();
            db.load_font_data(bytes.into());
        }

        self.font_family_by_name(&font_name)
            .ok_or_else(|| Error::FontLoadingFailed)
    }

    /// Get the font system.
    fn font_system(&mut self) -> &mut FontSystem {
        loop {
            match self {
                FontDatabase::FontDb { .. } => {
                    // Replace this database with the corresponding `FontSystem`.
                    let (locale, db) = match mem::replace(self, Self::Empty) {
                        FontDatabase::FontDb {
                            _locale: locale,
                            db,
                        } => (locale, db),
                        _ => unreachable!(),
                    };

                    // Construct the font system.
                    let font_system = FontSystem::new_with_locale_and_db(locale, db);
                    *self = FontDatabase::Cosmic(font_system);
                }
                FontDatabase::Cosmic(font_system) => return font_system,
                _ => unreachable!("cannot poll an empty hole"),
            }
        }
    }

    /// Get the underlying database.
    ///
    /// This does not mutate the structure.
    fn database(&self) -> &Database {
        match self {
            FontDatabase::FontDb { db, .. } => db,
            FontDatabase::Cosmic(cm) => cm.db(),
            _ => unreachable!("cannot poll an empty hole"),
        }
    }

    /// Get a mutable reference to the database.
    fn database_mut(&mut self) -> &mut Database {
        loop {
            match self {
                FontDatabase::FontDb { db, .. } => return db,
                FontDatabase::Cosmic(_) => {
                    // Replace this database with the corresponding `FontSystem`.
                    let font_system = match mem::replace(self, Self::Empty) {
                        FontDatabase::Cosmic(font_system) => font_system,
                        _ => unreachable!(),
                    };

                    // Construct the font system.
                    let (locale, db) = font_system.into_locale_and_db();
                    *self = FontDatabase::FontDb {
                        _locale: locale,
                        db,
                    };
                }
                _ => unreachable!("cannot poll an empty hole"),
            }
        }
    }
}

/// Intersection of two ranges.
fn intersect_ranges(a: &Range<usize>, b: &Range<usize>) -> Option<Range<usize>> {
    let start = a.start.max(b.start);
    let end = a.end.min(b.end);

    if start < end {
        Some(start..end)
    } else {
        None
    }
}

fn font_name(font: &[u8]) -> Result<String, Error> {
    // Parse it using ttf-parser
    let font = ttf_parser::Face::parse(font, 0).map_err(|e| Error::BackendError(e.into()))?;

    // Get the name with the main ID.
    let name = font
        .names()
        .into_iter()
        .find(|n| n.name_id == ttf_parser::name_id::FAMILY)
        .ok_or_else(|| Error::BackendError("font does not have a name with the main ID".into()))?;

    // TODO: Support macintosh encoding.
    name.to_string()
        .ok_or_else(|| Error::BackendError("font name is not valid UTF-16".into()))
}

fn points_to_pixels(points: f64) -> f64 {
    points * 96.0 / 72.0
}

fn cvt_color(p: piet::Color) -> cosmic_text::Color {
    let (r, g, b, a) = p.as_rgba8();
    cosmic_text::Color::rgba(r, g, b, a)
}

fn cvt_family(p: &piet::FontFamily) -> cosmic_text::Family<'_> {
    macro_rules! generic {
        ($piet:ident => $cosmic:ident) => {
            if p == &piet::FontFamily::$piet {
                return cosmic_text::Family::$cosmic;
            }
        };
    }

    if p.is_generic() {
        generic!(SERIF => Serif);
        generic!(SANS_SERIF => SansSerif);
        generic!(MONOSPACE => Monospace);
    }

    cosmic_text::Family::Name(p.name())
}

fn cvt_style(p: piet::FontStyle) -> cosmic_text::Style {
    use piet::FontStyle;

    match p {
        FontStyle::Italic => cosmic_text::Style::Italic,
        FontStyle::Regular => cosmic_text::Style::Normal,
    }
}

fn cvt_weight(p: piet::FontWeight) -> cosmic_text::Weight {
    cosmic_text::Weight(p.to_raw())
}
