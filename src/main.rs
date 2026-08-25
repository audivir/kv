use anyhow::{Context, Result};
use clap::Parser;
use kv::*;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;

type TempAndFinalOption = Option<(NamedTempFile, PathBuf)>;

fn prepare_writer(
    output: Option<String>,
    overwrite: bool,
) -> Result<(Box<dyn Write>, TempAndFinalOption)> {
    match output {
        Some(path_str) => {
            let path = PathBuf::from(path_str);

            let absolute_path = if !path.is_absolute() {
                std::env::current_dir()?.join(&path)
            } else {
                path.clone()
            };

            let parent = absolute_path.parent().context("Invalid output path")?;

            if !parent.exists() {
                anyhow::bail!("Output directory does not exist: {}", parent.display());
            }

            if absolute_path.exists() && !overwrite {
                anyhow::bail!(
                    "Output file already exists: {} (use --overwrite)",
                    path.display()
                );
            }

            let tempfile = NamedTempFile::new_in(parent).context(format!(
                "Failed to create temp file in {}",
                parent.display()
            ))?;

            let file = tempfile
                .as_file()
                .try_clone()
                .context("Failed to clone temp file")?;

            let writer: Box<dyn Write> = Box::new(BufWriter::new(file));
            Ok((writer, Some((tempfile, absolute_path))))
        }

        None => Ok((Box::new(BufWriter::new(io::stdout())), None)),
    }
}

fn main() -> Result<()> {
    let conf = Config::parse();

    if conf.plugins {
        open_config()?;
        return Ok(());
    }

    let term_size = get_term_size();

    // Detect TTY status
    let is_input_available = atty::isnt(atty::Stream::Stdin);

    let (writer, temp_output) = prepare_writer(conf.output.clone(), conf.overwrite)?;

    let code = run(
        writer,
        io::stderr(),
        io::stdin(),
        conf,
        term_size,
        is_input_available,
        None,
    )?;

    // Commit temp file only on success
    if let Some((tempfile, final_path)) = temp_output
        && code == 0
    {
        tempfile.persist(final_path)?;
    }

    std::process::exit(code);
}
