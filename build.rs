use std::env;
use std::path::{Path, PathBuf};

use burn_import::onnx::ModelGen;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/windows/sempal.rc");
    println!("cargo:rerun-if-changed=assets/logo3.ico");

    if let Err(error) = generate_burn_panns() {
        eprintln!("Failed to generate Burn PANNs model: {error}");
        std::process::exit(1);
    }

    if compiling_for_windows_target()
        && let Err(error) = compile_windows_resources()
    {
        eprintln!("Failed to embed Windows resources: {error}");
        std::process::exit(1);
    }
}

fn compiling_for_windows_target() -> bool {
    env::var("CARGO_CFG_TARGET_OS")
        .map(|target| target == "windows")
        .unwrap_or_else(|_| cfg!(target_os = "windows"))
}

fn compile_windows_resources() -> Result<(), Box<dyn std::error::Error>> {
    embed_resource::compile("build/windows/sempal.rc", embed_resource::NONE).manifest_optional()?;
    Ok(())
}

fn generate_burn_panns() -> Result<(), String> {
    println!("cargo:rerun-if-env-changed=SEMPAL_PANNS_ONNX_PATH");
    println!("cargo:rerun-if-env-changed=SEMPAL_MODELS_DIR");

    let onnx_path = panns_onnx_path();
    println!("cargo:rerun-if-changed={}", onnx_path.display());
    if !onnx_path.exists() {
        return Err(format!(
            "PANNs ONNX model not found at {}",
            onnx_path.display()
        ));
    }

    ModelGen::new()
        .input(
            onnx_path
                .to_str()
                .ok_or_else(|| "PANNs ONNX path contains invalid UTF-8".to_string())?,
        )
        .out_dir("burn_panns")
        .run_from_script();

    let out_dir = env::var("OUT_DIR").map_err(|err| err.to_string())?;
    let out_dir = Path::new(&out_dir).join("burn_panns");
    let stem = onnx_path
        .file_stem()
        .ok_or_else(|| "PANNs ONNX path missing file stem".to_string())?;
    let burnpack_path = out_dir.join(stem).with_extension("bpk");
    write_panns_paths(&out_dir, &burnpack_path)?;
    Ok(())
}

fn panns_onnx_path() -> PathBuf {
    if let Ok(path) = env::var("SEMPAL_PANNS_ONNX_PATH") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }
    if let Ok(path) = env::var("SEMPAL_MODELS_DIR") {
        if !path.trim().is_empty() {
            return PathBuf::from(path).join("panns_cnn14_16k.onnx");
        }
    }
    if let Some(appdata) = env::var_os("APPDATA") {
        return PathBuf::from(appdata)
            .join(".sempal")
            .join("models")
            .join("panns_cnn14_16k.onnx");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".sempal")
            .join("models")
            .join("panns_cnn14_16k.onnx");
    }
    PathBuf::from("panns_cnn14_16k.onnx")
}

fn write_panns_paths(out_dir: &Path, burnpack_path: &Path) -> Result<(), String> {
    let path_literal = format!("{:?}", burnpack_path.to_string_lossy());
    let contents = format!("pub const PANNS_BURNPACK_PATH: &str = {path_literal};\n");
    let out_path = out_dir.join("panns_paths.rs");
    std::fs::write(out_path, contents).map_err(|err| err.to_string())
}
