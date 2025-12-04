use std::{cell::RefCell, rc::Rc};

use hound::SampleFormat;
use slint::winit_030::{self, CustomApplicationHandler, EventResult};
use slint::{Rgb8Pixel, SharedPixelBuffer};

slint::slint! {
    export component HelloWorld inherits Window {
        width: 720px;
        height: 420px;
        in-out property <string> status_text: "Drop a .wav file";
        in-out property <image> waveform;

        VerticalLayout {
            spacing: 8px;
            padding: 12px;

            Rectangle {
                border-width: 1px;
                border-color: #404040;
                background: #101010;
                VerticalLayout {
                    spacing: 8px;
                    padding: 8px;

                    Text {
                        text: "Waveform Viewer";
                        horizontal-alignment: center;
                        color: #e0e0e0;
                        font-size: 18px;
                        width: parent.width;
                    }

                    Image {
                        source: root.waveform;
                        width: parent.width;
                        height: 260px;
                        image-fit: contain;
                        colorize: #00000000;
                    }
                }
            }

            Rectangle {
                height: 32px;
                background: #00000033;
                border-width: 1px;
                border-color: #303030;
                Text {
                    text: root.status_text;
                    vertical-alignment: center;
                    width: parent.width;
                    height: parent.height;
                    color: #d0d0d0;
                }
            }
        }
    }
}

#[derive(Clone)]
struct WaveformRenderer {
    width: u32,
    height: u32,
    background: Rgb8Pixel,
    foreground: Rgb8Pixel,
}

impl WaveformRenderer {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            background: Rgb8Pixel {
                r: 16,
                g: 16,
                b: 24,
            },
            foreground: Rgb8Pixel {
                r: 0,
                g: 200,
                b: 255,
            },
        }
    }

    fn empty_image(&self) -> slint::Image {
        self.render_waveform(&[])
    }

    fn load_samples(&self, bytes: &[u8]) -> Result<Vec<f32>, String> {
        let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))
            .map_err(|error| format!("Invalid wav: {error}"))?;
        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;

        let raw = match spec.sample_format {
            SampleFormat::Float => Self::read_float_samples(&mut reader, channels)?,
            SampleFormat::Int => {
                Self::read_int_samples(&mut reader, spec.bits_per_sample, channels)?
            }
        };

        Ok(raw)
    }

    fn read_float_samples(
        reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
        channels: usize,
    ) -> Result<Vec<f32>, String> {
        let raw: Vec<f32> = reader
            .samples::<f32>()
            .map(|s| s.map_err(|error| format!("Sample error: {error}")))
            .collect::<Result<_, _>>()?;
        Ok(Self::average_channels(raw, channels))
    }

    fn read_int_samples(
        reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
        bits_per_sample: u16,
        channels: usize,
    ) -> Result<Vec<f32>, String> {
        let scale = (1i64 << bits_per_sample.saturating_sub(1)).max(1) as f32;
        let raw: Vec<f32> = reader
            .samples::<i32>()
            .map(|s| {
                s.map(|v| v as f32 / scale)
                    .map_err(|error| format!("Sample error: {error}"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self::average_channels(raw, channels))
    }

    fn average_channels(raw: Vec<f32>, channels: usize) -> Vec<f32> {
        raw.chunks(channels)
            .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
            .collect()
    }

    fn render_waveform(&self, samples: &[f32]) -> slint::Image {
        let columns = self.sample_columns(samples);
        self.paint_image(&columns)
    }

    fn sample_columns(&self, samples: &[f32]) -> Vec<(f32, f32)> {
        let mut cols = vec![(0.0, 0.0); self.width as usize];
        if samples.is_empty() {
            return cols;
        }

        let max_amp = samples
            .iter()
            .fold(0.0_f32, |m, v| m.max(v.abs()))
            .max(1e-6);
        let chunk = (samples.len() / self.width as usize).max(1);

        for (x, col) in cols.iter_mut().enumerate() {
            let start = x * chunk;
            if start >= samples.len() {
                break;
            }
            let end = ((x + 1) * chunk).min(samples.len());
            let mut min: f32 = 1.0;
            let mut max: f32 = -1.0;
            for &sample in &samples[start..end] {
                let v = (sample / max_amp).clamp(-1.0, 1.0);
                min = min.min(v);
                max = max.max(v);
            }
            *col = (min, max);
        }

        cols
    }

    fn paint_image(&self, columns: &[(f32, f32)]) -> slint::Image {
        let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(self.width, self.height);
        self.fill_background(buffer.make_mut_slice());
        self.draw_columns(columns, buffer.make_mut_slice());
        slint::Image::from_rgb8(buffer)
    }

    fn fill_background(&self, pixels: &mut [Rgb8Pixel]) {
        for pixel in pixels {
            *pixel = self.background;
        }
    }

    fn draw_columns(&self, columns: &[(f32, f32)], pixels: &mut [Rgb8Pixel]) {
        let stride = self.width as usize;
        let mid = (self.height / 2) as f32;
        let limit = self.height.saturating_sub(1) as f32;

        for (x, (min, max)) in columns.iter().enumerate() {
            let top = (mid - max * (mid - 1.0)).clamp(0.0, limit) as u32;
            let bottom = (mid - min * (mid - 1.0)).clamp(0.0, limit) as u32;
            for y in top..=bottom {
                pixels[y as usize * stride + x] = self.foreground;
            }
        }
    }
}

#[derive(Clone)]
struct DropHandler {
    app: Rc<RefCell<Option<slint::Weak<HelloWorld>>>>,
    renderer: WaveformRenderer,
}

impl DropHandler {
    fn new(renderer: WaveformRenderer) -> Self {
        Self {
            app: Rc::new(RefCell::new(None)),
            renderer,
        }
    }

    fn set_app(&self, app: &HelloWorld) {
        self.app.replace(Some(app.as_weak()));
    }

    fn handle_drop(&self, path: &std::path::Path) {
        let Some(app) = self.app.borrow().as_ref().and_then(|a| a.upgrade()) else {
            return;
        };

        if !Self::is_wav(path) {
            app.set_status_text("Unsupported file type (please drop a .wav)".into());
            return;
        }

        match self.load_waveform_image(path) {
            Ok(image) => {
                let message = format!("Loaded {}", path.display());
                app.set_waveform(image);
                app.set_status_text(message.into());
            }
            Err(error) => app.set_status_text(error.into()),
        }
    }

    fn is_wav(path: &std::path::Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
    }

    fn load_waveform_image(&self, path: &std::path::Path) -> Result<slint::Image, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let samples = self.renderer.load_samples(&bytes)?;
        Ok(self.renderer.render_waveform(&samples))
    }
}

impl CustomApplicationHandler for DropHandler {
    fn window_event(
        &mut self,
        _event_loop: &winit_030::winit::event_loop::ActiveEventLoop,
        _window_id: winit_030::winit::window::WindowId,
        _winit_window: Option<&winit_030::winit::window::Window>,
        _slint_window: Option<&slint::Window>,
        event: &winit_030::winit::event::WindowEvent,
    ) -> EventResult {
        if let winit_030::winit::event::WindowEvent::DroppedFile(path_buf) = event {
            self.handle_drop(path_buf.as_path());
        }

        EventResult::Propagate
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let renderer = WaveformRenderer::new(680, 260);
    let drop_handler = DropHandler::new(renderer.clone());

    slint::BackendSelector::new()
        .require_wgpu_27(slint::wgpu_27::WGPUConfiguration::default())
        .with_winit_custom_application_handler(drop_handler.clone())
        .select()?;

    let app = HelloWorld::new()?;
    app.set_waveform(renderer.empty_image());
    drop_handler.set_app(&app);

    app.run()?;

    Ok(())
}
