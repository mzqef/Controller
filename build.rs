use std::env;
use std::fs;
use std::path::Path;

#[cfg(windows)]
extern crate winres;

fn main() {
    // Re-run this script if the config directory changes
    println!("cargo:rerun-if-changed=config");
    println!("cargo:rerun-if-changed=resources");
    
    // Generate gradient circle icon
    generate_icon();
    
    // Generate .ico file for Windows executable icon
    generate_ico();
    
    // Embed icon in Windows executable
    #[cfg(windows)]
    {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let ico_path = Path::new(&manifest_dir).join("resources").join("icon.ico");
        if ico_path.exists() {
            let mut res = winres::WindowsResource::new();
            res.set_icon(ico_path.to_str().unwrap());
            if let Err(e) = res.compile() {
                println!("cargo:warning=Failed to embed icon: {}", e);
            } else {
                println!("cargo:warning=Embedded icon into executable");
            }
        } else {
            println!("cargo:warning=icon.ico not found at {:?}", ico_path);
        }
    }

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

/// Generate a HEROIC rearing horse icon (64x64 with transparent background)
/// Thin detailed strokes with white-to-cyan gradient
fn generate_icon() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let resources_dir = Path::new(&manifest_dir).join("resources");
    let icon_path = resources_dir.join("icon.png");
    
    // Skip if icon already exists
    if icon_path.exists() {
        return;
    }
    
    // Create resources directory if needed
    if let Err(e) = fs::create_dir_all(&resources_dir) {
        println!("cargo:warning=Failed to create resources directory: {}", e);
        return;
    }
    
    // Generate 64x64 RGBA icon with heroic rearing horse
    let size = 64u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    
    // Helper to draw thin stroke with white-cyan gradient
    let draw_stroke = |rgba: &mut [u8], x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color_t: f32| {
        // Gradient: pure white (1.0) to electric cyan (0.0)
        let white = (255.0, 255.0, 255.0);
        let cyan = (0.0, 230.0, 255.0);
        let r = (cyan.0 + (white.0 - cyan.0) * color_t) as u8;
        let g = (cyan.1 + (white.1 - cyan.1) * color_t) as u8;
        let b = (cyan.2 + (white.2 - cyan.2) * color_t) as u8;
        
        let steps = ((x1 - x0).abs().max((y1 - y0).abs()) * 4.0) as i32 + 1;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let cx = x0 + (x1 - x0) * t;
            let cy = y0 + (y1 - y0) * t;
            let w = width * (0.7 + 0.4 * (t * std::f32::consts::PI).sin());
            
            for dy in (-(w as i32) - 1)..=(w as i32 + 1) {
                for dx in (-(w as i32) - 1)..=(w as i32 + 1) {
                    let px = (cx + dx as f32) as i32;
                    let py = (cy + dy as f32) as i32;
                    if px >= 0 && px < size as i32 && py >= 0 && py < size as i32 {
                        let dist = ((dx as f32).powi(2) + (dy as f32).powi(2)).sqrt();
                        if dist <= w {
                            let alpha = if dist > w - 0.8 { ((w - dist) / 0.8 * 255.0).min(255.0) as u8 } else { 255 };
                            let idx = ((py as u32 * size + px as u32) * 4) as usize;
                            let old_a = rgba[idx + 3] as u32;
                            let new_a = alpha as u32;
                            let out_a = old_a + new_a - (old_a * new_a / 255);
                            if out_a > 0 {
                                let blend = new_a as f32 / out_a as f32;
                                rgba[idx] = ((rgba[idx] as f32 * (1.0 - blend) + r as f32 * blend)) as u8;
                                rgba[idx + 1] = ((rgba[idx + 1] as f32 * (1.0 - blend) + g as f32 * blend)) as u8;
                                rgba[idx + 2] = ((rgba[idx + 2] as f32 * (1.0 - blend) + b as f32 * blend)) as u8;
                                rgba[idx + 3] = out_a.min(255) as u8;
                            }
                        }
                    }
                }
            }
        }
    };
    
    // HEROIC REARING HORSE - thin detailed strokes, white-cyan gradient
    // color_t: 1.0 = white (head/front), 0.0 = cyan (tail/back)
    
    // Back hooves (cyan)
    draw_stroke(&mut rgba, 42.0, 60.0, 44.0, 62.0, 1.0, 0.15);
    draw_stroke(&mut rgba, 54.0, 58.0, 56.0, 62.0, 1.0, 0.1);
    
    // Back legs (cyan-ish)
    draw_stroke(&mut rgba, 42.0, 48.0, 42.0, 60.0, 1.4, 0.2);
    draw_stroke(&mut rgba, 44.0, 40.0, 42.0, 48.0, 1.5, 0.25);
    draw_stroke(&mut rgba, 52.0, 46.0, 54.0, 58.0, 1.3, 0.15);
    draw_stroke(&mut rgba, 50.0, 38.0, 52.0, 46.0, 1.4, 0.2);
    
    // Hindquarters (transition)
    draw_stroke(&mut rgba, 48.0, 36.0, 54.0, 32.0, 1.8, 0.25);
    draw_stroke(&mut rgba, 46.0, 38.0, 50.0, 36.0, 1.6, 0.3);
    
    // Tail - flowing upward (cyan with gradient)
    draw_stroke(&mut rgba, 54.0, 32.0, 58.0, 26.0, 1.2, 0.1);
    draw_stroke(&mut rgba, 58.0, 26.0, 60.0, 18.0, 1.0, 0.05);
    draw_stroke(&mut rgba, 60.0, 18.0, 58.0, 10.0, 0.8, 0.0);
    draw_stroke(&mut rgba, 58.0, 10.0, 54.0, 4.0, 0.7, 0.0);
    draw_stroke(&mut rgba, 56.0, 28.0, 60.0, 22.0, 0.9, 0.08);
    draw_stroke(&mut rgba, 60.0, 22.0, 62.0, 14.0, 0.8, 0.02);
    draw_stroke(&mut rgba, 62.0, 14.0, 60.0, 6.0, 0.7, 0.0);
    draw_stroke(&mut rgba, 58.0, 24.0, 62.0, 18.0, 0.8, 0.05);
    draw_stroke(&mut rgba, 62.0, 18.0, 64.0, 10.0, 0.6, 0.0);
    
    // Body - muscular, angled upward
    draw_stroke(&mut rgba, 48.0, 36.0, 40.0, 30.0, 2.0, 0.35);
    draw_stroke(&mut rgba, 40.0, 30.0, 32.0, 24.0, 1.9, 0.45);
    draw_stroke(&mut rgba, 36.0, 32.0, 46.0, 38.0, 1.8, 0.35);
    draw_stroke(&mut rgba, 34.0, 28.0, 42.0, 34.0, 1.6, 0.4);
    
    // Chest (white-ish)
    draw_stroke(&mut rgba, 32.0, 24.0, 28.0, 28.0, 1.8, 0.55);
    draw_stroke(&mut rgba, 28.0, 28.0, 26.0, 34.0, 1.6, 0.6);
    
    // Front legs RAISED - heroic!
    draw_stroke(&mut rgba, 26.0, 34.0, 20.0, 40.0, 1.4, 0.65);
    draw_stroke(&mut rgba, 20.0, 40.0, 12.0, 42.0, 1.2, 0.7);
    draw_stroke(&mut rgba, 12.0, 42.0, 6.0, 38.0, 1.0, 0.75);
    draw_stroke(&mut rgba, 6.0, 38.0, 4.0, 34.0, 0.8, 0.8);
    draw_stroke(&mut rgba, 28.0, 30.0, 32.0, 38.0, 1.3, 0.6);
    draw_stroke(&mut rgba, 32.0, 38.0, 28.0, 46.0, 1.1, 0.65);
    draw_stroke(&mut rgba, 28.0, 46.0, 22.0, 44.0, 0.9, 0.7);
    
    // Neck - arched, powerful
    draw_stroke(&mut rgba, 32.0, 24.0, 28.0, 18.0, 1.7, 0.6);
    draw_stroke(&mut rgba, 28.0, 18.0, 26.0, 12.0, 1.6, 0.7);
    draw_stroke(&mut rgba, 26.0, 12.0, 28.0, 8.0, 1.5, 0.8);
    draw_stroke(&mut rgba, 30.0, 20.0, 28.0, 14.0, 1.4, 0.65);
    
    // Head - held high, noble
    draw_stroke(&mut rgba, 28.0, 8.0, 32.0, 6.0, 1.4, 0.85);
    draw_stroke(&mut rgba, 32.0, 6.0, 38.0, 8.0, 1.2, 0.9);
    draw_stroke(&mut rgba, 38.0, 8.0, 42.0, 10.0, 1.0, 0.95);
    draw_stroke(&mut rgba, 42.0, 10.0, 44.0, 12.0, 0.8, 1.0);
    // Jaw line
    draw_stroke(&mut rgba, 30.0, 10.0, 36.0, 12.0, 0.9, 0.88);
    // Eye
    draw_stroke(&mut rgba, 34.0, 8.0, 35.0, 9.0, 0.6, 0.0);
    // Nostril
    draw_stroke(&mut rgba, 43.0, 11.0, 44.0, 12.0, 0.5, 0.0);
    
    // Ears - alert
    draw_stroke(&mut rgba, 28.0, 8.0, 24.0, 2.0, 0.9, 0.9);
    draw_stroke(&mut rgba, 24.0, 2.0, 26.0, 4.0, 0.7, 0.85);
    draw_stroke(&mut rgba, 30.0, 6.0, 28.0, 0.0, 0.8, 0.92);
    draw_stroke(&mut rgba, 28.0, 0.0, 30.0, 2.0, 0.6, 0.88);
    
    // Mane - flowing dramatically (gradient to cyan)
    draw_stroke(&mut rgba, 26.0, 14.0, 18.0, 8.0, 1.0, 0.5);
    draw_stroke(&mut rgba, 18.0, 8.0, 10.0, 6.0, 0.8, 0.3);
    draw_stroke(&mut rgba, 10.0, 6.0, 4.0, 8.0, 0.6, 0.1);
    draw_stroke(&mut rgba, 24.0, 16.0, 14.0, 12.0, 1.1, 0.45);
    draw_stroke(&mut rgba, 14.0, 12.0, 6.0, 12.0, 0.9, 0.2);
    draw_stroke(&mut rgba, 6.0, 12.0, 2.0, 16.0, 0.7, 0.05);
    draw_stroke(&mut rgba, 22.0, 18.0, 12.0, 16.0, 1.0, 0.4);
    draw_stroke(&mut rgba, 12.0, 16.0, 4.0, 18.0, 0.8, 0.15);
    draw_stroke(&mut rgba, 28.0, 12.0, 20.0, 6.0, 0.9, 0.55);
    draw_stroke(&mut rgba, 20.0, 6.0, 12.0, 4.0, 0.7, 0.25);
    draw_stroke(&mut rgba, 30.0, 10.0, 24.0, 4.0, 0.8, 0.6);
    draw_stroke(&mut rgba, 24.0, 4.0, 16.0, 2.0, 0.6, 0.2);
    draw_stroke(&mut rgba, 20.0, 20.0, 8.0, 20.0, 0.9, 0.35);
    draw_stroke(&mut rgba, 8.0, 20.0, 2.0, 24.0, 0.7, 0.1);
    draw_stroke(&mut rgba, 18.0, 22.0, 6.0, 24.0, 0.8, 0.25);
    draw_stroke(&mut rgba, 6.0, 24.0, 2.0, 28.0, 0.6, 0.05);
    
    if let Err(e) = write_png(&icon_path, &rgba, size, size) {
        println!("cargo:warning=Failed to write icon: {}", e);
    } else {
        println!("cargo:warning=Generated HEROIC horse icon at {:?}", icon_path);
    }
}

/// Write RGBA data as PNG file (minimal implementation)
fn write_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> std::io::Result<()> {
    use std::io::Write;
    
    let mut file = fs::File::create(path)?;
    
    // PNG signature
    file.write_all(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])?;
    
    // IHDR chunk
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8);  // bit depth
    ihdr.push(6);  // color type (RGBA)
    ihdr.push(0);  // compression
    ihdr.push(0);  // filter
    ihdr.push(0);  // interlace
    write_chunk(&mut file, b"IHDR", &ihdr)?;
    
    // IDAT chunk (uncompressed using zlib stored blocks)
    let mut raw_data = Vec::new();
    for y in 0..height {
        raw_data.push(0); // No filter
        let start = (y * width * 4) as usize;
        let end = start + (width * 4) as usize;
        raw_data.extend_from_slice(&rgba[start..end]);
    }
    
    let compressed = zlib_compress_stored(&raw_data);
    write_chunk(&mut file, b"IDAT", &compressed)?;
    
    // IEND chunk
    write_chunk(&mut file, b"IEND", &[])?;
    
    Ok(())
}

fn write_chunk(file: &mut fs::File, chunk_type: &[u8; 4], data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    
    let len = data.len() as u32;
    file.write_all(&len.to_be_bytes())?;
    file.write_all(chunk_type)?;
    file.write_all(data)?;
    
    // CRC32 of type + data
    let crc = crc32(&[chunk_type.as_slice(), data].concat());
    file.write_all(&crc.to_be_bytes())?;
    
    Ok(())
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFFFFFFu32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Compress data using zlib stored blocks (no actual compression, just framing)
fn zlib_compress_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    
    // Zlib header
    out.push(0x78);
    out.push(0x01);
    
    // Split into 65535-byte blocks
    let chunks: Vec<&[u8]> = data.chunks(65535).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i == chunks.len() - 1;
        out.push(if is_last { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        let nlen = !len;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&nlen.to_le_bytes());
        out.extend_from_slice(chunk);
    }
    
    // Adler-32 checksum
    let adler = adler32(data);
    out.extend_from_slice(&adler.to_be_bytes());
    
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Generate .ico file from the PNG for Windows executable icon
fn generate_ico() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let resources_dir = Path::new(&manifest_dir).join("resources");
    let png_path = resources_dir.join("icon.png");
    let ico_path = resources_dir.join("icon.ico");
    
    // Skip if .ico already exists and is newer than PNG
    if ico_path.exists() {
        if let (Ok(png_meta), Ok(ico_meta)) = (fs::metadata(&png_path), fs::metadata(&ico_path)) {
            if let (Ok(png_time), Ok(ico_time)) = (png_meta.modified(), ico_meta.modified()) {
                if ico_time >= png_time {
                    return;
                }
            }
        }
    }
    
    // Read PNG file
    let png_data = match fs::read(&png_path) {
        Ok(data) => data,
        Err(_) => return,
    };
    
    // Create ICO file with embedded PNG (modern ICO format supports PNG directly)
    // ICO format:
    // - Header: 6 bytes (reserved=0, type=1, count=1)
    // - Directory entry: 16 bytes
    // - PNG data
    
    let mut ico = Vec::new();
    
    // ICO Header
    ico.extend_from_slice(&0u16.to_le_bytes()); // Reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // Type: 1 = ICO
    ico.extend_from_slice(&1u16.to_le_bytes()); // Count: 1 image
    
    // Directory entry
    ico.push(0); // Width: 0 means 256 (or use actual size, 64 fits in u8)
    ico.push(0); // Height: 0 means 256
    ico.push(0); // Color palette: 0 = no palette
    ico.push(0); // Reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // Color planes
    ico.extend_from_slice(&32u16.to_le_bytes()); // Bits per pixel
    ico.extend_from_slice(&(png_data.len() as u32).to_le_bytes()); // Image size
    ico.extend_from_slice(&22u32.to_le_bytes()); // Offset to image data (6 + 16 = 22)
    
    // PNG data
    ico.extend_from_slice(&png_data);
    
    if let Err(e) = fs::write(&ico_path, &ico) {
        println!("cargo:warning=Failed to write .ico file: {}", e);
    } else {
        println!("cargo:warning=Generated icon.ico at {:?}", ico_path);
    }
}
