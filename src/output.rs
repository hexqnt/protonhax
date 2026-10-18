use std::io::{self, Write};

use serde::Serialize;

pub fn print_json(value: &impl Serialize) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, value).map_err(io::Error::other)?;
    writeln!(output)
}
