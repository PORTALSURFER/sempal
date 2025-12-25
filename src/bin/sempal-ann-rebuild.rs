//! Developer utility to rebuild the ANN index from embeddings.

use rusqlite::Connection;
use std::path::PathBuf;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

struct Options {
    db_path: PathBuf,
}

fn run() -> Result<(), String> {
    let Some(options) = parse_args(std::env::args().skip(1).collect())? else {
        return Ok(());
    };
    let conn =
        Connection::open(&options.db_path).map_err(|err| format!("Open DB failed: {err}"))?;
    sempal::analysis::rebuild_ann_index(&conn)?;
    println!("Rebuilt ANN index for {}", options.db_path.display());
    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<Option<Options>, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(None);
    }
    let mut db_path = None;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--db" => {
                let value = it
                    .next()
                    .ok_or_else(|| "Missing value for --db".to_string())?;
                db_path = Some(PathBuf::from(value));
            }
            _ => return Err(format!("Unknown argument: {arg}")),
        }
    }
    let db_path = match db_path {
        Some(path) => path,
        None => {
            let root = sempal::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
            root.join(sempal::sample_sources::library::LIBRARY_DB_FILE_NAME)
        }
    };
    Ok(Some(Options { db_path }))
}

fn print_help() {
    println!("Usage: sempal-ann-rebuild [--db <path>]");
    println!();
    println!("Options:");
    println!("  --db <path>  Path to library.db (defaults to app data dir)");
}
