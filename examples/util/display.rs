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

//! Example harness for displaying a text layout.

use cosmic_text::SwashCache;
use piet::{kurbo::Vec2, TextLayout as _};
use piet_cosmic_text::{LineProcessor, Text, TextLayout};
use winit::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::EventLoop,
    platform::run_return::EventLoopExtRunReturn,
    window::WindowBuilder,
};

pub(super) fn run(mut f: impl FnMut(&mut Text, usize, usize) -> TextLayout + 'static) {
    tracing_subscriber::fmt::init();

    let mut width = 720;
    let mut height = 480;

    let mut event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("piet text example")
        .with_inner_size(LogicalSize::new(width as u32, height as u32))
        .build(&event_loop)
        .unwrap();

    let mut context = unsafe { softbuffer::GraphicsContext::new(&window, &window).unwrap() };

    let mut text = Text::new();
    let mut buffer = vec![0u32; width * height];

    let mut swash_cache = SwashCache::new();

    event_loop.run_return(move |event, _, control_flow| {
        control_flow.set_wait();

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => control_flow.set_exit(),

            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                width = size.width as usize;
                height = size.height as usize;
                buffer.resize(width * height, 0);
            }

            Event::RedrawEventsCleared => {
                // Fill buffer with white.
                buffer.fill(0x00FFFFFF);

                // If we aren't loaded yet, don't draw anything.
                if text.is_loaded() {
                    // Calculate text layout.
                    let text_layout = f(&mut text, width, height);
                    let mut lines = LineProcessor::new();

                    // Add an offset to the drawing.
                    let offset = {
                        let size = text_layout.size();
                        let x = ((width as f64 - size.width) / 2.0).max(0.0);
                        let y = ((height as f64 - size.height) / 2.0).max(0.0);
                        Vec2::new(x, y)
                    };

                    let mut pixmap = tiny_skia::PixmapMut::from_bytes(
                        bytemuck::cast_slice_mut(&mut buffer),
                        width as u32,
                        height as u32,
                    )
                    .unwrap();

                    // Draw pixels.
                    text.with_font_system_mut(|font_system| {
                        text_layout.buffer().draw(
                            font_system,
                            &mut swash_cache,
                            cosmic_text::Color::rgba(0, 0, 0, 0xFF),
                            |x, y, w, h, color| {
                                if x < 0 || y < 0 {
                                    return;
                                }

                                pixmap.fill_rect(
                                    tiny_skia::Rect::from_xywh(
                                        x as f32 + offset.x as f32,
                                        y as f32 + offset.y as f32,
                                        w as f32,
                                        h as f32,
                                    )
                                    .unwrap(),
                                    &tiny_skia::Paint {
                                        shader: tiny_skia::Shader::SolidColor({
                                            let [r, g, b, a] =
                                                [color.r(), color.g(), color.b(), color.a()];

                                            tiny_skia::Color::from_rgba8(r, g, b, a)
                                        }),
                                        ..Default::default()
                                    },
                                    tiny_skia::Transform::identity(),
                                    None,
                                );
                            },
                        );
                    });

                    // Draw lines.
                    text_layout
                        .layout_runs()
                        .flat_map(|run| {
                            let line_y = run.line_y;
                            run.glyphs.iter().map(move |glyph| (glyph, line_y))
                        })
                        .for_each(|(glyph, line_y)| {
                            lines.handle_glyph(
                                glyph,
                                line_y,
                                cosmic_text::Color::rgba(0, 0, 0, 0xFF),
                            );
                        });

                    lines.lines().into_iter().for_each(|line| {
                        tracing::trace!("Got line: {:?}", line);

                        let rect = {
                            let mut rect = line.into_rect();
                            rect.x0 += offset.x;
                            rect.y0 += offset.y;
                            rect.x1 += offset.x;
                            rect.y1 += offset.y;

                            tiny_skia::Rect::from_ltrb(
                                rect.x0 as f32,
                                rect.y0 as f32,
                                rect.x1 as f32,
                                rect.y1 as f32,
                            )
                            .unwrap()
                        };
                        let color = {
                            let (r, g, b, a) = line.color.as_rgba8();
                            tiny_skia::Color::from_rgba8(r, g, b, a)
                        };

                        pixmap.fill_rect(
                            rect,
                            &tiny_skia::Paint {
                                shader: tiny_skia::Shader::SolidColor(color),
                                ..Default::default()
                            },
                            tiny_skia::Transform::identity(),
                            None,
                        );
                    });
                } else {
                    // Wait a second then try again.
                    control_flow.set_wait_timeout(std::time::Duration::from_secs(1));
                }

                // Push buffer to softbuffer.
                context.set_buffer(&buffer, width as _, height as _);
            }

            _ => (),
        }
    });
}
