//! An example for drawing some basic text.

use cosmic_text::SwashCache;

use piet::{FontFamily, Text as _, TextLayoutBuilder as _};
use piet_cosmic_text::Text;

use winit::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    platform::run_return::EventLoopExtRunReturn,
    window::WindowBuilder,
};

fn main() {
    let mut width = 720;
    let mut height = 480;

    let mut event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("piet text example")
        .with_inner_size(LogicalSize::new(width as u32, height as u32))
        .build(&event_loop)
        .unwrap();

    let mut context = unsafe { softbuffer::GraphicsContext::new(&window, &window).unwrap() };

    let text = Text::new();
    let mut buffer = vec![0u32; width * height];

    text.with_font_system({
        let mut text = text.clone();
        move |font_system| {
            let mut swash_cache = SwashCache::new(font_system);

            event_loop.run_return(move |event, _, control_flow| {
                *control_flow = ControlFlow::Wait;

                match event {
                    Event::WindowEvent {
                        event: WindowEvent::CloseRequested,
                        ..
                    } => *control_flow = ControlFlow::Exit,

                    Event::WindowEvent {
                        event: WindowEvent::Resized(size),
                        ..
                    } => {
                        width = size.width as usize;
                        height = size.height as usize;
                        buffer.resize(width * height, 0);
                    }

                    Event::RedrawRequested(_) => {
                        // Fill buffer with white.
                        buffer.fill(0x00FFFFFF);

                        // Calculate text layout.
                        let text_layout = text
                            .new_text_layout("Line #1\nLine #2\nمرحبا بالعالم\n💀 💀 💀\nThis is an exceptionally long line! foobar foobar foobar foobar")
                            .font(FontFamily::SANS_SERIF, 24.0)
                            .max_width(width as _)
                            .build()
                            .unwrap();

                        // Draw pixels.
                        text_layout.buffer().draw(
                            &mut swash_cache,
                            cosmic_text::Color::rgba(0, 0, 0, 0xFF),
                            |x, y, _, _, color| {
                                if x < 0 || y < 0 {
                                    return;
                                }

                                let pixel_start = (y as usize * width) + x as usize;
                                let rgba = {
                                    let alpha_filter = (color.a() as f32) / 255.0;

                                    let cvt = |x| {
                                        (((x as f32) * alpha_filter)
                                            + (255.0 * (1.0 - alpha_filter)))
                                            as u32
                                    };

                                    ((cvt(color.r())) << 16)
                                        | ((cvt(color.g())) << 8)
                                        | (cvt(color.b()))
                                };

                                buffer[pixel_start] = rgba;
                            },
                        );

                        // Push buffer to softbuffer.
                        context.set_buffer(&buffer, width as _, height as _);
                    }

                    _ => (),
                }
            });
        }
    });
}
