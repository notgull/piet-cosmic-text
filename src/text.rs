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

//! The `Text` API, the root of the system.

use crate::export_work::ExportWork;
use crate::text_layout::{InkRectangleState, TextLayout};
use crate::text_layout_builder::TextLayoutBuilder;
use crate::{channel, FontError, STANDARD_DPI};

#[cfg(feature = "embed_fonts")]
use crate::embedded_fonts;

#[cfg(not(feature = "rayon"))]
use crate::export_work::CurrentThread;

#[cfg(feature = "rayon")]
use crate::export_work::Rayon;

use cosmic_text as ct;
use event_listener::Event;

use ct::fontdb::{Family, Query, ID as FontId};
use ct::{Attrs, AttrsOwned, BufferLine, FontSystem};

use piet::{Error, FontFamily, TextStorage};

use std::cell::{Cell, RefCell, RefMut};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::sync::Arc;

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

    /// Cache the ink rectangle calculation state.
    ink: RefCell<InkRectangleState>,
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

/// A guard for accessing the font system.
///
/// This is essentially a thread-unsafe mutex that uses `EventListener` for notifications.
pub(crate) struct FontSystemGuard<'a> {
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
        self.font_db_free.notify(1);
    }
}

/// Either a `FontSystem` or a handle that can be resolved to one.
#[allow(clippy::large_enum_variant)] // No need to box the FontSystem since it will be used soon.
pub(crate) enum DelayedFontSystem {
    /// The real font system.
    Real(FontSystemAndDefaults),

    /// We are waiting for a font system to be loaded.
    Waiting(channel::Receiver<FontSystemAndDefaults>),
}

impl fmt::Debug for DelayedFontSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Real(fs) => f
                .debug_struct("FontSystem")
                .field("db", fs.system.db())
                .field("locale", &fs.system.locale())
                .field("default_fonts", &fs.default_fonts)
                .finish_non_exhaustive(),
            Self::Waiting(_) => f.write_str("<waiting for availability>"),
        }
    }
}

impl DelayedFontSystem {
    /// Get the font system.
    pub(crate) fn get(&mut self) -> Option<&mut FontSystemAndDefaults> {
        loop {
            match self {
                Self::Real(system) => return Some(system),
                Self::Waiting(channel) => {
                    // Try to wait on the channel without blocking.
                    *self = Self::Real(channel.try_recv()?);
                }
            }
        }
    }

    /// Wait until the font system is loaded.
    pub(crate) async fn wait(&mut self) -> &mut FontSystemAndDefaults {
        loop {
            match self {
                Self::Real(font_system) => return font_system,
                Self::Waiting(recv) => *self = Self::Real(recv.recv().await),
            }
        }
    }

    /// Wait until the font system is loaded, blocking redux.
    pub(crate) fn wait_blocking(&mut self) -> &mut FontSystemAndDefaults {
        loop {
            match self {
                Self::Real(font_system) => return font_system,
                Self::Waiting(recv) => *self = Self::Real(recv.recv_blocking()),
            }
        }
    }
}

/// The `FontSystem` and the bundled default fonts.
pub(crate) struct FontSystemAndDefaults {
    /// The underlying font system.
    pub(crate) system: FontSystem,

    /// Fonts to use if there are no font matches.
    ///
    /// This contains the default serif, sans-serif and monospace fonts, as well as
    /// any fonts embedded into the executable.
    pub(crate) default_fonts: Vec<FontId>,
}

impl FontSystemAndDefaults {
    /// Modify the attributes until they match at least one font.
    pub(crate) fn fix_attrs(&mut self, attrs: Attrs<'_>) -> AttrsOwned {
        let mut owned = AttrsOwned::new(attrs);
        let original = attrs;

        // If we have a font, great!
        if !self.system.get_font_matches(attrs).is_empty() {
            return owned;
        }

        // If we don't, iterate over the default fonts until we do.
        for _ in 0..2 {
            for &default_font in &self.default_fonts {
                if let Some(font) = self.system.db().face(default_font) {
                    for (name, _) in font.families.clone() {
                        owned.family_owned = ct::FamilyOwned::Name(name);
                        if !self.system.get_font_matches(owned.as_attrs()).is_empty() {
                            return owned;
                        }
                    }
                }
            }

            // Reset the style info to as blank as possible and try again.
            owned.style = ct::Style::Normal;
            owned.weight = ct::Weight::NORMAL;
        }

        // Give up.
        warn!("no fonts match attributes: {:?}", original);
        AttrsOwned::new(original)
    }
}

impl Text {
    /// Borrow the inner `DelayedFontSystem`.
    pub(crate) fn borrow_font_system(&self) -> Option<FontSystemGuard<'_>> {
        self.0.borrow_font_system()
    }

    /// Borrow the ink rectangle state.
    pub(crate) fn borrow_ink(&self) -> RefMut<'_, InkRectangleState> {
        self.0.ink.borrow_mut()
    }

    /// Take the inner `BufferLine` buffer.
    pub(crate) fn take_buffer(&self) -> Vec<BufferLine> {
        self.0.buffer.replace(Vec::new())
    }

    /// Set the inner `BufferLine` buffer.
    pub(crate) fn set_buffer(&self, buffer: Vec<BufferLine>) {
        self.0.buffer.replace(buffer);
    }

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
            #[allow(unused_mut)]
            let mut fs = FontSystem::new();
            let mut defaults = vec![];

            // Embed the fonts into the system.
            #[cfg(feature = "embed_fonts")]
            {
                match embedded_fonts::load_embedded_font_data(&mut fs) {
                    Ok(mut ids) => defaults.append(&mut ids),
                    Err(_err) => {
                        error!("failed to load embedded font data: {}", _err);
                    }
                }
            }

            // Add default serif fonts to the defaults.
            {
                let mut add_defaults = |family: Family<'_>| {
                    if let Some(font) = fs.db().query(&Query {
                        families: &[family],
                        ..Default::default()
                    }) {
                        defaults.insert(0, font);
                    } else {
                        warn!("failed to find default font for family {:?}", family);
                    }
                };

                add_defaults(Family::SansSerif);
                add_defaults(Family::Serif);
                add_defaults(Family::Monospace);
            }

            send.send(FontSystemAndDefaults {
                system: fs,
                default_fonts: defaults,
            });
        });

        Self::with_delayed_font_system(DelayedFontSystem::Waiting(recv))
    }

    /// Create a new `Text` renderer from an existing `FontSystem`.
    pub fn from_font_system(font_system: FontSystem) -> Self {
        let defaults = {
            let load_default_family = |family: Family<'_>| {
                font_system.db().query(&Query {
                    families: &[family],
                    ..Default::default()
                })
            };

            let mut defaults = vec![];
            defaults.extend(load_default_family(Family::SansSerif));
            defaults.extend(load_default_family(Family::Serif));
            defaults.extend(load_default_family(Family::Monospace));
            defaults
        };

        Self::with_delayed_font_system(DelayedFontSystem::Real(FontSystemAndDefaults {
            system: font_system,
            default_fonts: defaults,
        }))
    }

    fn with_delayed_font_system(font_db: DelayedFontSystem) -> Self {
        Self(Rc::new(Inner {
            font_db: RefCell::new(font_db),
            font_db_free: Event::new(),
            buffer: Cell::new(Vec::new()),
            dpi: Cell::new(STANDARD_DPI),
            ink: RefCell::new(InkRectangleState::new()),
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
    /// Loading new fonts or calculating the text bounding box while this function is in use will
    /// result in an error.
    pub fn with_font_system_mut<R>(&self, f: impl FnOnce(&mut FontSystem) -> R) -> Option<R> {
        let mut font_db = self.0.borrow_font_system()?;
        font_db.get().map(|fs| f(&mut fs.system))
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
            let name = db.system.db().family_name(&name);
            if name == family_name {
                return Some(piet_name);
            }
        }

        // Get the font family.
        let family = Family::Name(family_name);
        let name = db.system.db().family_name(&family);

        // Look for the font with that name.
        let font = db
            .system
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
                .system
                .db_mut()
                .load_font_source(ct::fontdb::Source::Binary(Arc::new(data.to_vec())));

            // For simplicity, just take the first ID if this is a font collection.
            match ids.len() {
                0 => {
                    error!("font collection contained no fonts");
                    return Err(Error::FontLoadingFailed);
                }
                1 => ids[0],
                _len => {
                    warn!("received font collection of length {_len}, only selecting first font");
                    ids[0]
                }
            }
        };

        // Get the font back.
        let font = db
            .system
            .db()
            .face(id)
            .ok_or_else(|| Error::FontLoadingFailed)?;
        Ok(FontFamily::new_unchecked(font.families[0].0.as_str()))
    }

    fn new_text_layout(&mut self, text: impl TextStorage) -> Self::TextLayoutBuilder {
        TextLayoutBuilder::new(self.clone(), text)
    }
}
