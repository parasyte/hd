use self::grapheme::Char;
use self::group::{Group, Kind};
use colorz::{mode::set_coloring_mode_from_env, Colorize as _};
use error_iter::ErrorIter as _;
use onlyargs::OnlyArgs as _;
use onlyargs_derive::OnlyArgs;
use onlyerror::Error;
use std::fmt::{self, Write as _};
use std::io::{self, Read, Write as _};
use std::{fs::File, path::PathBuf, process::ExitCode, str::FromStr};

mod grapheme;
mod group;

#[derive(OnlyArgs)]
#[footer = "Environment variables:"]
#[footer = "  - NO_COLOR: Disable colors entirely"]
#[footer = "  - ALWAYS_COLOR: Always enable colors"]
#[footer = ""]
#[footer = "  - CLICOLOR_FORCE: Same as ALWAYS_COLOR"]
#[footer = "  - FORCE_COLOR: Same as ALWAYS_COLOR"]
struct Args {
    /// Number of bytes to print per row.
    #[default(16)]
    width: usize,

    /// Number of bytes to group within a row.
    #[default(2)]
    group: usize,

    /// Numeric classification for character table.
    /// Prints bytes in cyan that match one of the following numeric classes:
    ///  - `o`, `oct`, or `octal`: `/[0-7]+/`
    ///  - `d`, `dec`, or `decimal`: `/[\d]+/`
    ///  - `h`, `x`, `hex`, or `hexadecimal`: `/[a-f\d]+/i`
    ///
    #[default("decimal")]
    numeric: String,

    /// A list of file paths to read.
    #[positional]
    input: Vec<PathBuf>,
}

/// All possible errors that can be reported to the user.
#[derive(Debug, Error)]
enum Error {
    /// CLI argument parsing error
    Cli(#[from] onlyargs::CliError),

    /// Width must be in range `2 <= width < 4096`
    Width,

    /// Grouping must not be larger than width
    Grouping,

    /// Unable to read file
    #[error("Unable to read file: {1:?}")]
    File(#[source] io::Error, PathBuf),

    /// Unknown numeric class
    #[error("Unknown numeric class: `{0}`")]
    UnknownNumeric(String),

    /// I/O error
    Io(#[from] io::Error),

    /// String formatting error
    Fmt(#[from] fmt::Error),
}

impl Error {
    /// Check if the error was caused by CLI inputs.
    fn is_cli(&self) -> bool {
        use Error::*;

        matches!(
            self,
            Cli(_) | Width | Grouping | File(_, _) | UnknownNumeric(_)
        )
    }
}

fn main() -> ExitCode {
    set_coloring_mode_from_env();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if error.is_cli() {
                let _ = writeln!(io::stderr(), "{}", Args::HELP);
            }

            let _ = writeln!(io::stderr(), "{}: {error}", "Error".bright_red());
            for source in error.sources().skip(1) {
                let _ = writeln!(io::stderr(), "  {}: {source}", "Caused by".bright_yellow());
            }

            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    let args: Args = onlyargs::parse()?;
    let width = args.width;
    let group = args.group;
    let numeric = args.numeric.parse()?;
    let mut printer = Printer::new(width, group, numeric)?;

    if args.input.is_empty() {
        // Read from stdin.
        printer.pretty_hex(&mut io::stdin())?;
    } else {
        // Read file paths.
        let show_header = args.input.len() > 1;
        for path in args.input.into_iter() {
            if show_header && writeln!(io::stdout(), "\n[{}]", path.display().yellow()).is_err() {
                std::process::exit(1);
            }
            let mut file = File::open(&path).map_err(|err| Error::File(err, path.to_path_buf()))?;
            printer.pretty_hex(&mut file)?;
        }
    }

    Ok(())
}

/// Numeric context for byte classification.
#[derive(Copy, Clone)]
enum Numeric {
    Octal,
    Decimal,
    Hexadecimal,
}

impl FromStr for Numeric {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "o" | "oct" | "octal" => Ok(Self::Octal),
            "d" | "dec" | "decimal" => Ok(Self::Decimal),
            "h" | "x" | "hex" | "hexadecimal" => Ok(Self::Hexadecimal),
            _ => Err(Error::UnknownNumeric(s.to_string())),
        }
    }
}

/// Row printer. Pretty prints byte slices one row at a time.
struct Printer {
    /// Number of bytes per row.
    width: usize,

    /// Number of bytes to group within a row.
    group: usize,

    /// Numeric classification for character table.
    numeric: Numeric,

    /// Total number of columns to print for the hex digits in each row.
    max: usize,

    /// Internal state for printing rows and grouping bytes.
    state: PrinterState,
}

#[derive(Default)]
struct PrinterState {
    addr: usize,
    column: usize,
    hex: String,
    table: String,
    hex_group: String,
    table_group: String,
}

impl Printer {
    /// Create a new row printer with width and group counts.
    ///
    /// # Errors
    ///
    /// - [`Error::Width`]: `width` is greater than 4096.
    /// - [`Error::Grouping`]: `group` is greater than `width`.
    fn new(width: usize, group: usize, numeric: Numeric) -> Result<Self, Error> {
        if width <= 1 || width > 4096 {
            Err(Error::Width)
        } else if group > width {
            Err(Error::Grouping)
        } else {
            Ok(Self {
                width,
                group,
                numeric,
                max: padding(group, width),
                state: Default::default(),
            })
        }
    }

    /// Pretty print a [`Reader`] as hex bytes.
    fn pretty_hex<R>(&mut self, reader: &mut R) -> Result<(), Error>
    where
        R: Read,
    {
        let mut buf = [0; 4096];

        loop {
            // Read as much as possible, appending to buffer.
            let size = reader.read(&mut buf)?;
            if size == 0 {
                break;
            }

            // Print bytes grouped by classification.
            let mut start = 0;
            while start < size {
                let group = Group::gather(&buf[start..size], self.numeric);
                start += group.span.bytes.len();
                self.format_group(group)?;
            }
        }

        // Print any remaining row.
        if self.state.column > 0 {
            self.print_row()?;
        }

        Ok(())
    }

    /// Format a classified group of bytes.
    fn format_group(&mut self, group: Group<'_>) -> Result<(), Error> {
        for (i, byte) in group.span.bytes.iter().enumerate() {
            // Write byte group separator.
            if self.state.column % self.group == 0 {
                self.state.hex_group.write_char(' ')?;
            }

            // Write hex.
            write!(&mut self.state.hex_group, "{byte:02x}")?;

            // Write character table.
            let ch = match group.kind {
                Kind::Printable | Kind::Numeric => Some(*byte as char),
                Kind::Graphemes => match group.span.as_char(i, self.state.column, self.width) {
                    Char::Cluster(cluster) => {
                        self.state.table_group.write_str(cluster)?;
                        None
                    }
                    Char::Space => Some(' '),
                    Char::Skip => None,
                },
                Kind::Control | Kind::Invalid => Some('.'),
            };
            if let Some(ch) = ch {
                self.state.table_group.write_char(ch)?;
            }

            self.state.column += 1;
            if self.state.column == self.width {
                self.colorize_group(group.kind)?;
                self.print_row()?;
            }
        }

        if self.state.column > 0 {
            self.colorize_group(group.kind)?;
        }

        Ok(())
    }

    // Colorize formatted group.
    fn colorize_group(&mut self, kind: Kind) -> Result<(), Error> {
        let hex = &mut self.state.hex;
        let table = &mut self.state.table;
        let row_group = &self.state.hex_group;
        let table_group = &self.state.table_group;
        match kind {
            Kind::Control => {
                write!(hex, "{}", row_group.bright_yellow())?;
                write!(table, "{}", table_group.bright_yellow())?;
            }
            Kind::Printable => {
                write!(hex, "{}", row_group.bright_green())?;
                write!(table, "{}", table_group.bright_green())?;
            }
            Kind::Numeric => {
                write!(hex, "{}", row_group.bright_cyan())?;
                write!(table, "{}", table_group.bright_cyan())?;
            }
            Kind::Graphemes => {
                write!(hex, "{}", row_group.green().bold())?;
                write!(table, "{}", table_group.green().bold())?;
            }
            Kind::Invalid => {
                write!(hex, "{}", row_group.bright_red())?;
                write!(table, "{}", table_group.bright_red())?;
            }
        }

        self.state.hex_group.clear();
        self.state.table_group.clear();

        Ok(())
    }

    // Print a complete row.
    fn print_row(&mut self) -> Result<(), Error> {
        let written = writeln!(
            io::stdout(),
            "{addr}:{hex}{hex_pad} | {table}{table_pad} |",
            addr = self.pretty_addr(),
            hex = self.state.hex,
            hex_pad = " ".repeat(self.max - padding(self.group, self.state.column)),
            table = self.state.table,
            table_pad = " ".repeat(self.width - self.state.column),
        );

        // Exit process if the stdout pipe was closed.
        if written.is_err() {
            std::process::exit(1);
        }

        self.state.column = 0;
        self.state.addr += self.width;
        self.state.hex.clear();
        self.state.table.clear();

        Ok(())
    }

    // Return the address as a formatted and colorized string.
    fn pretty_addr(&self) -> colorz::StyledValue<String, colorz::ansi::BrightBlue> {
        let a = self.state.addr >> 48;
        let b = (self.state.addr >> 32) & 0xffff;
        let c = (self.state.addr >> 16) & 0xffff;
        let d = self.state.addr & 0xffff;

        format!("{:04x}_{:04x}_{:04x}_{:04x}", a, b, c, d).into_bright_blue()
    }
}

/// Compute the number of columns needed to print a byte slice of the given length as grouped hex
/// bytes.
fn padding(group: usize, length: usize) -> usize {
    length * 2 + length.div_ceil(group)
}
