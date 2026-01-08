use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // Re-run this script if the config directory changes
    println!("cargo:rerun-if-changed=config");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let profile = env::var("PROFILE").unwrap();
    
    // Source directory: <project_root>/config
    let src_dir = Path::new(&manifest_dir).join("config");
    
    // Destination directory: <project_root>/target/<profile>/config
    // Note: This assumes the standard Cargo target directory structure.
    // If CARGO_TARGET_DIR is set to something else, this might need adjustment.
    let target_dir = Path::new(&manifest_dir).join("target").join(&profile);
    let dest_dir = target_dir.join("config");

    if !src_dir.exists() {
        println!("cargo:warning=Config directory not found at {:?}", src_dir);
        return;
    }

    // Create the destination directory
    if let Err(e) = fs::create_dir_all(&dest_dir) {
        println!("cargo:warning=Failed to create config directory at {:?}: {}", dest_dir, e);
        return;
    }

    // Copy files from source to destination, but skip copying if destination exists and contents match
    match fs::read_dir(&src_dir) {
        Ok(entries) => {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_file() {
                        let file_name = entry.file_name();
                        let dest_path = dest_dir.join(&file_name);

                        let need_copy = if dest_path.exists() {
                            // Quick size check then content compare
                            let src_meta = fs::metadata(&path);
                            let dst_meta = fs::metadata(&dest_path);
                            if let (Ok(s), Ok(d)) = (src_meta, dst_meta) {
                                if s.len() == d.len() {
                                    // Compare bytes
                                    match (fs::read(&path), fs::read(&dest_path)) {
                                        (Ok(sa), Ok(da)) => sa != da,
                                        _ => true,
                                    }
                                } else { true }
                            } else { true }
                        } else { true };

                        if need_copy {
                            if let Err(e) = fs::copy(&path, &dest_path) {
                                println!("cargo:warning=Failed to copy {:?} to {:?}: {}", file_name, dest_path, e);
                            }
                        }
                    }
                }
            }
        },
        Err(e) => {
            println!("cargo:warning=Failed to read config directory: {}", e);
        }
    }
}
