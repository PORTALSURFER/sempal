use std::path::PathBuf;

use sempal::model_setup::{PannsSetupOptions, ensure_panns_burnpack};

fn main() {
    let mut options = PannsSetupOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--onnx-url" => {
                if let Some(value) = args.next() {
                    options.onnx_url = Some(value);
                }
            }
            "--onnx-sha256" => {
                if let Some(value) = args.next() {
                    options.onnx_sha256 = Some(value);
                }
            }
            "--models-dir" => {
                if let Some(value) = args.next() {
                    options.models_dir = Some(PathBuf::from(value));
                }
            }
            "--force" => {
                options.force = true;
            }
            "--help" | "-h" => {
                print_help();
                return;
            }
            _ => {}
        }
    }

    match ensure_panns_burnpack(options) {
        Ok(path) => {
            println!("PANNs burnpack ready: {}", path.display());
        }
        Err(err) => {
            eprintln!("Failed to prepare PANNs burnpack: {err}");
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!(
        "Usage: sempal-model-setup [--onnx-url <url>] [--onnx-sha256 <hex>] [--models-dir <path>] [--force]"
    );
}
