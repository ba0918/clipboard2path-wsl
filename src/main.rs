mod domain;
mod infra;
mod service;

use std::path::Path;
use std::process;

use domain::wsl_detect;
use infra::clipboard::WlClipboard;
use infra::file_system::RealFileWriter;
use service::converter::{ConvertService, SystemTimestamp};

fn main() {
    // WSL2 check
    let proc_version = match std::fs::read_to_string("/proc/version") {
        Ok(content) => content,
        Err(e) => {
            eprintln!("error: failed to read /proc/version: {e}");
            process::exit(1);
        }
    };

    if !wsl_detect::is_wsl2(&proc_version) {
        eprintln!("error: this tool requires WSL2 environment");
        process::exit(1);
    }

    // Validate output directory
    let base_dir = Path::new("/tmp");
    if let Err(e) = domain::path_gen::validate_output_dir(base_dir) {
        eprintln!("error: invalid output directory: {e}");
        process::exit(1);
    }

    // DI assembly
    let service = ConvertService::new(WlClipboard, WlClipboard, RealFileWriter, SystemTimestamp);

    // Execute conversion
    match service.convert_once(base_dir) {
        Ok(path) => {
            eprintln!("saved: {}", path.display());
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}
