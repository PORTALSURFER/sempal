mod app;
mod audio;
mod file_browser;
mod sample_sources;
mod ui;
mod waveform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run()
}
