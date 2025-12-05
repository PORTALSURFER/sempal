mod app;
mod audio;
mod file_browser;
mod ui;
mod waveform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run()
}
