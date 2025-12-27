use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/windows/sempal.rc");
    println!("cargo:rerun-if-changed=assets/logo3.ico");

    if let Err(error) = stage_bundled_panns() {
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

fn stage_bundled_panns() -> Result<(), String> {
    println!("cargo:rerun-if-env-changed=SEMPAL_PANNS_MODEL_RS_PATH");
    println!("cargo:rerun-if-env-changed=SEMPAL_MODELS_DIR");

    let out_dir = env::var("OUT_DIR").map_err(|err| err.to_string())?;
    let out_dir = Path::new(&out_dir).join("burn_panns");
    std::fs::create_dir_all(&out_dir).map_err(|err| err.to_string())?;

    let model_rs = panns_model_rs_path();
    println!("cargo:rerun-if-changed={}", model_rs.display());
    if !model_rs.exists() {
        return Err(format!(
            "PANNs model source not found at {}",
            model_rs.display()
        ));
    }
    let out_rs = out_dir.join("panns_cnn14_16k.rs");
    std::fs::copy(&model_rs, &out_rs).map_err(|err| err.to_string())?;

    let burnpack_path = panns_burnpack_path();
    println!("cargo:rerun-if-changed={}", burnpack_path.display());
    write_panns_paths(&out_dir, &burnpack_path)?;
    Ok(())
}

fn panns_model_rs_path() -> PathBuf {
    if let Ok(path) = env::var("SEMPAL_PANNS_MODEL_RS_PATH") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }
    if let Ok(path) = env::var("SEMPAL_MODELS_DIR") {
        if !path.trim().is_empty() {
            return PathBuf::from(path).join("panns_cnn14_16k.rs");
        }
    }
    PathBuf::from("assets/ml/panns_cnn14_16k/panns_cnn14_16k.rs")
}

fn panns_burnpack_path() -> PathBuf {
    if let Ok(path) = env::var("SEMPAL_MODELS_DIR") {
        if !path.trim().is_empty() {
            return PathBuf::from(path).join("panns_cnn14_16k.bpk");
        }
    }
    PathBuf::from("assets/ml/panns_cnn14_16k/panns_cnn14_16k.bpk")
}

fn write_panns_paths(out_dir: &Path, burnpack_path: &Path) -> Result<(), String> {
    let path_literal = format!("{:?}", burnpack_path.to_string_lossy());
    let contents = format!("pub const PANNS_BURNPACK_PATH: &str = {path_literal};\n");
    let out_path = out_dir.join("panns_paths.rs");
    std::fs::write(out_path, contents).map_err(|err| err.to_string())
}
