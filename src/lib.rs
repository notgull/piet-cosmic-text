//! An implementation of [`piet`]'s text API using [`cosmic-text`].
//!
//! [`piet`]: https://docs.rs/piet
//! [`cosmic-text`]: https://docs.rs/cosmic-text

#![forbid(unsafe_code, future_incompatible, rust_2018_idioms)]

use cosmic_text::fontdb::{Database, Family};
use cosmic_text::{
    Buffer, BufferLine, CacheKey as GlyphKey, Font, FontSystem, LayoutGlyph, LayoutRunIter, Metrics,
};

use piet::{Color, Error, FontFamily, TextAlignment, TextAttribute};

use std::cell::{Cell, Ref, RefCell};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt;
use std::mem;
use std::ops::{Bound, Range, RangeBounds};
use std::rc::Rc;

/// The text implementation entry point.
///
/// # Limitations
///
/// `load_from_data` should not be called while `TextLayout` objects are alive; otherwise, a panic will
/// occur.
#[derive(Clone)]
pub struct Text {
    /// Font database.
    font_db: Rc<RefCell<FontDatabase>>,
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
        Self {
            font_db: Rc::new(RefCell::new(FontDatabase::Cosmic(font_system))),
        }
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
        self.font_db
            .try_borrow()
            .ok()
            .and_then(|font_db| font_db.font_family_by_name(family_name))
    }

    fn load_font(&mut self, data: &[u8]) -> Result<FontFamily, Error> {
        self.font_db
            .try_borrow_mut()
            .map_err(|_| {
                Error::BackendError(
                    "tried to load font while TextLayoutBuilder/TextLayout is alive".into(),
                )
            })
            .and_then(|mut font_db| font_db.load_font(data))
    }

    fn new_text_layout(&mut self, text: impl piet::TextStorage) -> Self::TextLayoutBuilder {
        let text = {
            let str = text.as_str().to_string();
            Rc::from(str)
        };

        TextLayoutBuilder {
            handle: self.clone(),
            string: text,
            default_attributes: vec![],
            range_attributes: HashMap::new(),
            alignment: TextAlignment::Start,
            max_width: f64::INFINITY,
        }
    }
}

/// The text layout builder used by the [`RenderContext`].
#[derive(Debug, Clone)]
pub struct TextLayoutBuilder {
    /// Handle to the original `Text` object.
    handle: Text,

    /// The string we're laying out.
    string: Rc<str>,

    /// The default text attributes.
    default_attributes: Vec<TextAttribute>,

    /// The range attributes.
    range_attributes: HashMap<Range<usize>, Vec<TextAttribute>>,

    /// The alignment.
    alignment: TextAlignment,

    /// The allowed buffer size.
    max_width: f64,
}

impl piet::TextLayoutBuilder for TextLayoutBuilder {
    type Out = TextLayout;

    fn alignment(mut self, alignment: TextAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    fn max_width(mut self, width: f64) -> Self {
        self.max_width = width;
        self
    }

    fn default_attribute(mut self, attribute: impl Into<TextAttribute>) -> Self {
        self.default_attributes.push(attribute.into());
        self
    }

    fn range_attribute(
        mut self,
        range: impl RangeBounds<usize>,
        attribute: impl Into<TextAttribute>,
    ) -> Self {
        let start = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n + 1,
            Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            Bound::Included(&n) => n + 1,
            Bound::Excluded(&n) => n,
            Bound::Unbounded => self.string.len(),
        };

        let range = start..end;

        let attributes = match self.range_attributes.entry(range) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(Vec::new()),
        };

        attributes.push(attribute.into());

        self
    }

    fn build(self) -> Result<Self::Out, Error> {
        let metrics = Metrics::new(todo!(), todo!());

        // Build the self-referencing structure.
        let inner = SelfRefBufferTryBuilder::<_, _, Error> {
            text: self.handle.clone(),
            font_db_builder: |text| Ok(text.font_db.borrow()),
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

                Ok(Buffer::new(font_system, metrics))
            },
        }
        .try_build()?;

        Ok(TextLayout {
            string: self.string,
            buffer: inner, 
        })
    }
}

/// A text layout.
#[derive(Clone)]
pub struct TextLayout {
    /// The original string.
    string: Rc<str>,

    /// A wrapper around a buffer but with the lifetime requirement elided.
    buffer: SelfRefBuffer,
}

#[ouroboros::self_referencing]
struct SelfRefBuffer {
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

impl Clone for SelfRefBuffer {
    fn clone(&self) -> Self {
        SelfRefBufferBuilder {
            text: self.borrow_text().clone(),
            font_db_builder: |text| text.font_db.borrow(),
            buffer_builder: |font_db| {
                let this_buffer = self.borrow_buffer();

                let metrics = this_buffer.metrics();
                let (width, height) = this_buffer.size();

                let mut new_buffer = Buffer::new(
                    match &**font_db {
                        FontDatabase::Cosmic(font_system) => font_system,
                        _ => unreachable!(),
                    },
                    metrics,
                );
                new_buffer.set_size(width, height);

                new_buffer
            },
        }
        .build()
    }
}

impl TextLayout {
    /// Get a reference to the inner `Buffer`.
    pub fn buffer(&self) -> &Buffer<'_> {
        self.buffer.borrow_buffer()
    }
}

impl piet::TextLayout for TextLayout {
    fn size(&self) -> piet::kurbo::Size {
        todo!()
    }

    fn trailing_whitespace_width(&self) -> f64 {
        todo!()
    }

    fn image_bounds(&self) -> piet::kurbo::Rect {
        todo!()
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
        todo!()
    }

    fn line_count(&self) -> usize {
        self.buffer().layout_runs().count()
    }

    fn hit_test_point(&self, point: piet::kurbo::Point) -> piet::HitTestPoint {
        todo!()
    }

    fn hit_test_text_position(&self, idx: usize) -> piet::HitTestPosition {
        todo!()
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
