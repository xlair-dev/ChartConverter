use std::{
    env,
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use chart::ChartMode;
use chart_converter::{Format, convert_bytes_with_modes};

const USAGE: &str = "Usage: chart-converter [OPTIONS] <INPUT> <OUTPUT>

Convert between C2S, SUS, and UGC chart files. Use '-' for stdin or stdout.
Formats are inferred from file extensions unless explicitly specified.

Options:
  --input-format <FORMAT>   Input format: c2s, sus, or ugc
  --output-format <FORMAT>  Output format: c2s, sus, or ugc
  --mode <MODE>             Set both modes: normal or xlair
  --input-mode <MODE>       Input interpretation mode: normal or xlair
  --output-mode <MODE>      Output interpretation mode: normal or xlair
  -h, --help                Print help
  -V, --version             Print version";

struct Args {
    input: PathBuf,
    output: PathBuf,
    input_format: Option<Format>,
    output_format: Option<Format>,
    mode: Option<ChartMode>,
    input_mode: Option<ChartMode>,
    output_mode: Option<ChartMode>,
}

fn main() -> ExitCode {
    match run(env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("chart-converter: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let args = match parse_args(arguments)? {
        Some(args) => args,
        None => return Ok(()),
    };
    let input_format = args
        .input_format
        .or_else(|| infer_format(&args.input))
        .ok_or_else(|| "cannot infer input format; use --input-format".to_owned())?;
    let output_format = args
        .output_format
        .or_else(|| infer_format(&args.output))
        .ok_or_else(|| "cannot infer output format; use --output-format".to_owned())?;
    let mode = args.mode.unwrap_or(ChartMode::Normal);
    let input_mode = args.input_mode.unwrap_or(mode);
    let output_mode = args.output_mode.unwrap_or(mode);
    let input = read_input(&args.input)
        .map_err(|error| format!("failed to read '{}': {error}", args.input.display()))?;
    let output =
        convert_bytes_with_modes(input_format, input_mode, output_format, output_mode, &input)
            .map_err(|error| error.to_string())?;
    write_output(&args.output, output.as_bytes())
        .map_err(|error| format!("failed to write '{}': {error}", args.output.display()))
}

fn parse_args(arguments: impl IntoIterator<Item = OsString>) -> Result<Option<Args>, String> {
    let mut input = None;
    let mut output = None;
    let mut input_format = None;
    let mut output_format = None;
    let mut mode = None;
    let mut input_mode = None;
    let mut output_mode = None;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        let Some(option) = argument.to_str() else {
            set_path(&mut input, &mut output, PathBuf::from(argument))?;
            continue;
        };
        match option {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("chart-converter {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--input-format" => {
                input_format = Some(parse_format(&next_value(&mut arguments, option)?)?);
            }
            "--output-format" => {
                output_format = Some(parse_format(&next_value(&mut arguments, option)?)?);
            }
            "--mode" => mode = Some(parse_mode(&next_value(&mut arguments, option)?)?),
            "--input-mode" => {
                input_mode = Some(parse_mode(&next_value(&mut arguments, option)?)?);
            }
            "--output-mode" => {
                output_mode = Some(parse_mode(&next_value(&mut arguments, option)?)?);
            }
            _ if option.starts_with('-') && option != "-" => {
                return Err(format!("unknown option '{option}'\n\n{USAGE}"));
            }
            _ => set_path(&mut input, &mut output, PathBuf::from(argument))?,
        }
    }

    let Some(input) = input else {
        return Err(format!("missing input path\n\n{USAGE}"));
    };
    let Some(output) = output else {
        return Err(format!("missing output path\n\n{USAGE}"));
    };
    Ok(Some(Args {
        input,
        output,
        input_format,
        output_format,
        mode,
        input_mode,
        output_mode,
    }))
}

fn set_path(
    input: &mut Option<PathBuf>,
    output: &mut Option<PathBuf>,
    path: PathBuf,
) -> Result<(), String> {
    if input.is_none() {
        *input = Some(path);
        Ok(())
    } else if output.is_none() {
        *output = Some(path);
        Ok(())
    } else {
        Err(format!("too many paths\n\n{USAGE}"))
    }
}

fn next_value(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &str,
) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("missing value for {option}"))?
        .into_string()
        .map_err(|_| format!("value for {option} must be valid UTF-8"))
}

fn parse_format(value: &str) -> Result<Format, String> {
    Format::from_extension(value)
        .ok_or_else(|| format!("unsupported format '{value}'; expected c2s, sus, or ugc"))
}

fn parse_mode(value: &str) -> Result<ChartMode, String> {
    match value.to_ascii_lowercase().as_str() {
        "normal" => Ok(ChartMode::Normal),
        "xlair" => Ok(ChartMode::Xlair),
        _ => Err(format!(
            "unsupported mode '{value}'; expected normal or xlair"
        )),
    }
}

fn infer_format(path: &std::path::Path) -> Option<Format> {
    path.extension()?.to_str().and_then(Format::from_extension)
}

fn read_input(path: &std::path::Path) -> io::Result<Vec<u8>> {
    if path == std::path::Path::new("-") {
        let mut input = Vec::new();
        io::stdin().read_to_end(&mut input)?;
        Ok(input)
    } else {
        fs::read(path)
    }
}

fn write_output(path: &std::path::Path, output: &[u8]) -> io::Result<()> {
    if path == std::path::Path::new("-") {
        let mut stdout = io::stdout().lock();
        stdout.write_all(output)?;
        stdout.flush()
    } else {
        fs::write(path, output)
    }
}
