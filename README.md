# Simple Rust Backup (srb)

`simple-rust-backup` (`srb`) is a command-line utility that performs differential backups by copying new or modified files from a source directory to a target directory. It efficiently synchronizes files, ensuring that only files that have changed are copied, saving time and resources.

## Building the Program

To build the program from source, you need to have [Rust](https://www.rust-lang.org/tools/install) installed.

Clone the repository and compile the executable binary:
```bash
git clone https://github.com/010josh010/simple-rust-backup.git &&
cd simple-rust-backup &&
cargo build --bin srb
```
Add the executable to your `PATH`

## Usage

Run the program using the `srb` or `simple-rust-backup` executable, specifying the source and target directories.

    ./srb -s /path/to/source_dir -t /path/to/target_dir

### Command-Line Options

- `-s`, `--source_dir` : Source directory to backup
- `-t`, `--target_dir` : Target directory where backup will be stored
- `-h`, `--help`       : Show help message and exit

### Examples

    # macOS and Linux
    srb -s /home/user/documents -t /mnt/backup/documents

    # Windows
    srb -s C:\Users\Username\Documents -t D:\Backup\Documents

---
