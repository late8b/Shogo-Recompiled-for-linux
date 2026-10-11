//! Decode every extracted asset with the native Rust decoders and report
//! counts, so we can confirm parity with the Python pipeline
//! (538/538 ABC, 3641/3675 DTX).
//!
//! Usage:
//!   cargo run --example decode_all -- <extracted/SHOGO>

use std::fs;
use std::path::Path;

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| "../extracted/SHOGO".into());
    let root = Path::new(&root);

    let (mut abc_ok, mut abc_fail) = (0u32, 0u32);
    let (mut dtx_ok, mut dtx_fail) = (0u32, 0u32);
    let mut abc_tris = 0u64;
    let mut first_abc_fail = Vec::new();
    let mut first_dtx_fail = Vec::new();

    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if ext == "abc" {
                match fs::read(&path) {
                    Ok(data) => {
                        let name = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        match shogo_assets::abc::decode(&data, &name) {
                            Ok(m) => {
                                abc_ok += 1;
                                abc_tris += m.tri_count() as u64;
                            }
                            Err(e) => {
                                abc_fail += 1;
                                if first_abc_fail.len() < 5 {
                                    first_abc_fail.push(format!("{path:?}: {e}"));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        abc_fail += 1;
                        if first_abc_fail.len() < 5 {
                            first_abc_fail.push(format!("{path:?}: read error {e}"));
                        }
                    }
                }
            } else if ext == "dtx" {
                match fs::read(&path) {
                    Ok(data) => match shogo_assets::dtx::decode(&data) {
                        Ok(_t) => dtx_ok += 1,
                        Err(e) => {
                            dtx_fail += 1;
                            if first_dtx_fail.len() < 5 {
                                first_dtx_fail.push(format!("{path:?}: {e}"));
                            }
                        }
                    },
                    Err(_) => dtx_fail += 1,
                }
            }
        }
    }

    println!("ABC  ok={abc_ok}  fail={abc_fail}  total_tris={abc_tris}");
    for s in &first_abc_fail {
        println!("   ABC fail: {s}");
    }
    println!("DTX  ok={dtx_ok}  fail={dtx_fail}");
    for s in &first_dtx_fail {
        println!("   DTX fail: {s}");
    }
}
