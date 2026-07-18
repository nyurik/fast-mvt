use std::io::{self, Write as _};
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use fast_mvt::{MvtReaderRef, MvtResult};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Dump a vector tile in a human-readable form.
    Dump {
        /// MVT file to dump.
        file: PathBuf,
    },
}

fn main() -> MvtResult<()> {
    run(Cli::parse())
}

fn run(cli: Cli) -> MvtResult<()> {
    match cli.command {
        Command::Dump { file } => dump_file(file),
    }
}

fn dump_file(file: PathBuf) -> MvtResult<()> {
    let data = std::fs::read(file)?;
    let reader = MvtReaderRef::new(&data)?;
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    // The human-readable dump lives in the library as `MvtReaderRef`'s `Debug`.
    // Its lines are already newline-terminated, so use `write!`, not `writeln!`.
    write!(out, "{reader:?}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn run_dump_reads_tile_file() {
        let bytes = Vec::new();
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&bytes).unwrap();

        run(Cli {
            command: Command::Dump {
                file: file.path().to_path_buf(),
            },
        })
        .unwrap();
    }
}
