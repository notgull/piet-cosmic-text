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

//! Tiny oneshot channel.
//!
//! Using this means we don't have to take a dependency on `async-channel`.

use event_listener::Event;

use std::mem;
use std::sync::{Arc, Mutex};

/// Create a new channel.
pub(crate) fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let channel = Arc::new(Channel {
        val: Mutex::new(State::Empty),
        waker: Event::new(),
    });
    (Sender(channel.clone()), Receiver(channel))
}

/// The sender side of the channel.
pub(crate) struct Sender<T>(Arc<Channel<T>>);

impl<T> Sender<T> {
    /// Send a value.
    pub(crate) fn send(&self, val: T) {
        let mut lock = self.0.val.lock().unwrap();
        *lock = State::Full(val);
        self.0.waker.notify(1);
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut lock = self.0.val.lock().unwrap();
        match &mut *lock {
            State::Full(_) => {}
            _ => {
                *lock = State::Closed;
                self.0.waker.notify(1);
            }
        }
    }
}

/// The receiver side of the channel.
pub(crate) struct Receiver<T>(Arc<Channel<T>>);

impl<T> Receiver<T> {
    /// Try to receive a value.
    pub(crate) fn try_recv(&self) -> Option<T> {
        let mut lock = self.0.val.lock().unwrap();
        match &mut *lock {
            State::Full(_) => {
                if let State::Full(val) = mem::replace(&mut *lock, State::Empty) {
                    Some(val)
                } else {
                    unreachable!()
                }
            }
            State::Empty => None,
            State::Closed => panic!("channel is closed"),
        }
    }

    /// Wait for a value.
    pub(crate) async fn recv(&self) -> T {
        loop {
            // Try to take out the value.
            if let Some(value) = self.try_recv() {
                return value;
            }

            // Register a listener.
            let listener = self.0.waker.listen();

            // Try again.
            if let Some(value) = self.try_recv() {
                return value;
            }

            // Wait for a value.
            listener.await;
        }
    }

    /// Wait for a value, blocking edition.
    pub(crate) fn recv_blocking(&self) -> T {
        loop {
            // Try to take out the value.
            if let Some(value) = self.try_recv() {
                return value;
            }

            // Register a listener.
            let listener = self.0.waker.listen();

            // Try again.
            if let Some(value) = self.try_recv() {
                return value;
            }

            // Wait for a value.
            listener.wait();
        }
    }
}

/// The inner channel object.
struct Channel<T> {
    /// The value.
    val: Mutex<State<T>>,

    /// Wakes up when a value is pushed.
    waker: Event,
}

/// State of the channel.
enum State<T> {
    /// The channel is empty.
    Empty,

    /// The channel has a value.
    Full(T),

    /// The channel is closed.
    Closed,
}
