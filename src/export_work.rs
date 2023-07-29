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

//! A trait for exporting work to other threads.

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
