mod app;
mod audio;
mod sample_sources;
mod selection;
mod ui;
mod waveform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run()
}
