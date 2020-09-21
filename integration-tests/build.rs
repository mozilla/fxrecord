use std::env;
use std::fs::File;
use std::io;
use std::path::Path;
use std::process::Command;

use zip::write::FileOptions;
use zip::ZipWriter;

fn main() {
    // We need to build fakefox in a separate target directory, or else nested
    // cargo build will hang forever waiting for a lock on the target directory.
    let fakefox_target_path = env::current_dir()
        .expect("no cwd")
        .parent()
        .expect("no parent diorectory")
        .join("target")
        .join("nested");

    let cargo = env::var("CARGO").expect("no CARGO during cargo build");
    let cargo_status = Command::new(&cargo)
        .args(&["build", "-p", "fakefox", "--target-dir"])
        .arg(&fakefox_target_path)
        .status()
        .expect("could not execute `cargo build -p fakefox`.");
    assert!(
        cargo_status.success(),
        "Failed to run `cargo build -p fakefox`."
    );

    let out_dir = env::var("OUT_DIR").expect("no OUT_DIR during cargo build");
    let out_dir = Path::new(&out_dir);

    let fakefox_path = fakefox_target_path.join("debug").join("fakefox.exe");
    let zip_path = out_dir.join("firefox.zip");

    let mut zip_file = File::create(&zip_path).expect("could not create firefox.zip");
    let mut fakefox_file = File::open(&fakefox_path).expect("could not open fakefox.exe");

    let mut zip = ZipWriter::new(&mut zip_file);
    zip.add_directory("firefox", FileOptions::default())
        .unwrap();
    zip.start_file("firefox/firefox.exe", FileOptions::default())
        .unwrap();
    io::copy(&mut fakefox_file, &mut zip).unwrap();
    zip.finish().unwrap();

    println!("wrote firefox.zip to {}", zip_path.display());
}
