use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::cmp::Reverse;
use walkdir::WalkDir;

use clap::{Parser, ArgAction};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// A simple Rust program for differential backup
#[derive(Parser, Debug)]
#[command(name = "simple-rust-backup")]
#[command(author = "Joshua Vaughn <https://github.com/010josh010>")]
#[command(version = "0.1.0")]
#[command(
    about = "Performs differential backups from a source directory to a target directory.",
    long_about = None
)]
struct Args {
    /// Source directory to backup
    #[arg(short = 's', long)]
    source_dir: String,

    /// Target directory where backup will be stored
    #[arg(short = 't', long)]
    target_dir: String,

    /// Also delete any file that is present in the target but absent in the source
    #[arg(long, action = ArgAction::SetTrue)]
    delete: bool,
}

fn main() {
    // Parse command‑line arguments
    let args = Args::parse();

    let source_dir = Path::new(&args.source_dir);
    let target_dir = Path::new(&args.target_dir);

    // ── sanity‑checking source / target ──────────────────────────────────────
    if !source_dir.is_dir() {
        eprintln!("Source directory does not exist or is not a directory.");
        return;
    }
    if target_dir.exists() {
        if !target_dir.is_dir() {
            eprintln!("Target path exists but is not a directory.");
            return;
        }
    } else if let Err(e) = fs::create_dir_all(target_dir) {
        eprintln!("Failed to create target directory: {e}");
        return;
    }

    // ── collect all files from source ───────────────────────────────────────
    let mut files_to_process = Vec::new();
    for entry in WalkDir::new(source_dir) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Error reading entry: {e}");
                continue;
            }
        };
        if entry.path().is_file() {
            files_to_process.push(entry);
        }
    }

    // ── progress bars setup ─────────────────────────────────────────────────
    let mp = MultiProgress::new();
    let pb = mp.add(ProgressBar::new(files_to_process.len() as u64));
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] \
                 [{bar:40.cyan/blue}] {pos}/{len} {msg}",
            )
            .expect("Failed to set progress bar template")
            .progress_chars("#>-"),
    );

    // ── 1. copy / update phase ──────────────────────────────────────────────
    for entry in files_to_process {
        let path = entry.path();

        // relative path inside the tree
        let relative_path = match path.strip_prefix(source_dir) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error computing relative path: {e}");
                pb.inc(1);
                continue;
            }
        };
        let target_path = target_dir.join(relative_path);

        // decide whether we need to copy
        let should_copy = if target_path.exists() {
            let source_mod = match fs::metadata(path).and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Error reading source mtime: {e}");
                    pb.inc(1);
                    continue;
                }
            };
            let target_mod = match fs::metadata(&target_path).and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Error reading target mtime: {e}");
                    pb.inc(1);
                    continue;
                }
            };
            source_mod > target_mod
        } else {
            true
        };

        if should_copy {
            // make sure the parent dir exists
            if let Some(parent) = target_path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Error creating directories: {e}");
                    pb.inc(1);
                    continue;
                }
            }

            // remove read‑only bit on Windows so we can overwrite
            #[cfg(target_os = "windows")]
            {
                if target_path.exists() {
                    if let Err(e) = remove_readonly_attribute(&target_path) {
                        eprintln!("Error removing read‑only attribute: {e}");
                        pb.inc(1);
                        continue;
                    }
                }
            }

            if let Err(e) = copy_with_progress(path, &target_path, relative_path, &mp) {
                eprintln!("Error copying file: {e}");
                pb.inc(1);
                continue;
            }
        }
        pb.inc(1);
    }

    // ── 2. optional purge phase ─────────────────────────────────────────────
    if args.delete {
       println!("Cleaning up orphan files …");
        if let Err(e) = purge_orphans(source_dir, target_dir) {
            eprintln!("Deletion phase finished with errors: {e}");
        }
    }

    pb.finish_with_message("Backup completed.");
}

#[cfg(target_os = "windows")]
fn remove_readonly_attribute(path: &Path) -> io::Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    if perms.readonly() {
        perms.set_readonly(false);
        fs::set_permissions(path, perms)?;
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

    let pb = mp.add(ProgressBar::new(total_size));
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.blue} {msg:40} [{bar:40.magenta/blue}] {bytes}/{total_bytes}")
            .expect("Failed to set progress bar template"),
    );
    pb.set_message(relative_path.to_string_lossy().into_owned());

    let mut buffer = [0u8; 8 * 1024];
    let mut bytes_copied = 0;

    loop {
        let n = src_file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        dst_file.write_all(&buffer[..n])?;
        bytes_copied += n as u64;
        pb.set_position(bytes_copied);
    }

    pb.finish_and_clear();
    Ok(())
}

/// Walk the *target* tree and delete anything that has no counterpart
/// in *source*. Removes empty directories after files are gone.
fn purge_orphans(source_root: &Path, target_root: &Path) -> io::Result<()> {
    // ── 1. collect every entry in target_root ───────────────────────────────
    let mut entries: Vec<_> = WalkDir::new(target_root)
        .into_iter()
        .filter_map(|e| e.ok())
        // skip the root itself (depth 0) so we never try to delete target_root
        .filter(|e| e.depth() > 0)
        .collect();

    // ── 2. sort deepest‑first so we delete files before their parent dirs ───
    entries.sort_by_key(|e| Reverse(e.depth()));

    let mut deleted: Vec<String> = Vec::new();

    // ── 3. walk the list, delete where counterpart is missing ───────────────
    for entry in entries {
        let rel = entry
            .path()
            .strip_prefix(target_root)
            .expect("target_root prefix");

        // counterpart path in the source tree
        let counterpart = source_root.join(rel);

        if counterpart.exists() {
            continue; // keep anything that still exists in source
        }

        #[cfg(target_os = "windows")]
        let _ = remove_readonly_attribute(entry.path());

        let res = if entry.path().is_file() {
            fs::remove_file(entry.path())
        } else {
            // might fail if dir not empty; that’s fine
            fs::remove_dir(entry.path())
        };

        match res {
            Ok(_) => deleted.push(rel.display().to_string()),
            Err(e) => eprintln!("⚠️  Failed to delete {}: {}", rel.display(), e),
        }
    }

    // ── 4. summary ──────────────────────────────────────────────────────────
    if deleted.is_empty() {
        println!("➡️  No orphan files to delete.");
    } else {
        println!("🗑️  Deleted {} orphan item(s):", deleted.len());
        for path in deleted {
            println!("  • {}", path);
        }
    }

    Ok(())
}