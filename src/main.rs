use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use walkdir::WalkDir;

use clap::Parser;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};


/// A simple Rust program for differential backup
#[derive(Parser, Debug)]
#[command(name = "simple-rust-backup")]
#[command(author = "Joshua Vaughn <https://github.com/010josh010>")]
#[command(version = "0.1.0")]
#[command(about = "Performs differential backups from a source directory to a target directory.", long_about = None)]
struct Args {
    /// Source directory to backup
    #[arg(short = 's', long)]
    source_dir: String,

    /// Target directory where backup will be stored
    #[arg(short = 't', long)]
    target_dir: String,
}

fn main() {
    // Parse command-line arguments using clap
    let args = Args::parse();

    let source_dir = Path::new(&args.source_dir);
    let target_dir = Path::new(&args.target_dir);

    // Validate source directory
    if !source_dir.is_dir() {
        eprintln!("Source directory does not exist or is not a directory.");
        return;
    }

    // Validate and prepare the target directory
    if target_dir.exists() {
        if !target_dir.is_dir() {
            eprintln!("Target path exists but is not a directory.");
            return;
        }
    } else {
        // Attempt to create the target directory
        if let Err(e) = fs::create_dir_all(target_dir) {
            eprintln!("Failed to create target directory: {}", e);
            return;
        }
    }

    // Collect all files to process
    let mut files_to_process = Vec::new();
    for entry in WalkDir::new(source_dir) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Error reading entry: {}", e);
                continue;
            }
        };

        let path = entry.path();
        if path.is_file() {
            files_to_process.push(entry);
        }
    }

    // Create a MultiProgress to manage multiple progress bars
    let mp = MultiProgress::new();

    // Create the overall progress bar
    let pb = mp.add(ProgressBar::new(files_to_process.len() as u64));
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .expect("Failed to set progress bar template")
            .progress_chars("#>-"),
    );

    // Process files
    for entry in files_to_process {
        let path = entry.path();

        // Compute the relative path from the source directory
        let relative_path = match path.strip_prefix(source_dir) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error computing relative path: {}", e);
                pb.inc(1);
                continue;
            }
        };

        let target_path = target_dir.join(relative_path);

        // Determine if the file should be copied
        let should_copy = if target_path.exists() {
            // Compare modification times
            let source_modified = match fs::metadata(path).and_then(|m| m.modified()) {
                Ok(time) => time,
                Err(e) => {
                    eprintln!("Error reading source modification time: {}", e);
                    pb.inc(1);
                    continue;
                }
            };
            let target_modified = match fs::metadata(&target_path).and_then(|m| m.modified()) {
                Ok(time) => time,
                Err(e) => {
                    eprintln!("Error reading target modification time: {}", e);
                    pb.inc(1);
                    continue;
                }
            };

            source_modified > target_modified
        } else {
            true
        };

        if should_copy {
            // Ensure the target directory exists
            if let Some(parent) = target_path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Error creating directories: {}", e);
                    pb.inc(1);
                    continue;
                }
            }

            // Remove read-only attribute on Windows
            #[cfg(target_os = "windows")]
            {
                if target_path.exists() {
                    if let Err(e) = remove_readonly_attribute(&target_path) {
                        eprintln!("Error removing read-only attribute: {}", e);
                        pb.inc(1);
                        continue;
                    }
                }
            }

            // Copy the file with progress
            if let Err(e) = copy_with_progress(path, &target_path, relative_path, &mp) {
                eprintln!("Error copying file: {}", e);
                pb.inc(1);
                continue;
            }
        
        } 

        pb.inc(1);
    }

    pb.finish_with_message("Backup completed.");
}

#[cfg(target_os = "windows")]
fn remove_readonly_attribute(target_path: &Path) -> std::io::Result<()> {
    let metadata = fs::metadata(target_path)?;
    let mut permissions = metadata.permissions();

    // Check if the file is read-only
    if permissions.readonly() {
        permissions.set_readonly(false);
        fs::set_permissions(target_path, permissions)?;
    }

    Ok(())
}

fn copy_with_progress(
    src: &Path,
    dst: &Path,
    relative_path: &Path,
    mp: &MultiProgress,
) -> io::Result<()> {
    let metadata = fs::metadata(src)?;
    let total_size = metadata.len();

    let mut src_file = File::open(src)?;
    let mut dst_file = File::create(dst)?;

    // Create the per-file progress bar using MultiProgress
    let pb = mp.add(ProgressBar::new(total_size));
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} Backing up {msg}\n  {bar:40.cyan/blue} {bytes}/{total_bytes} ({bytes_per_sec}, ETA: {eta})")
            .expect("Failed to set per-file progress bar template")
            .progress_chars("#>-"),
    );

    // Set the message to the filename
    pb.set_message(format!("{:?}", relative_path));

    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = src_file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        dst_file.write_all(&buffer[..bytes_read])?;
        pb.inc(bytes_read as u64);
    }

    pb.finish_and_clear(); // Clear the per-file progress bar and message when done

    Ok(())
}