use clap::{Arg, ArgAction, command};
use encoding_rs::Encoding;
use failure::ResultExt;
use std::path::PathBuf;
use std::str::FromStr;

use crate::errors::{InputArgumentsError, InputArgumentsErrorKind};

pub struct Arguments {
    pub reference_file_path: PathBuf,
    pub incorrect_file_path: PathBuf,
    pub output_file_path: PathBuf,

    pub interval: i64,

    pub split_penalty: f64,

    pub sub_fps_inc: f64,
    pub sub_fps_ref: f64,

    pub allow_negative_timestamps: bool,

    /// having a value of `None` means autodetect encoding
    pub encoding_ref: Option<&'static Encoding>,
    pub encoding_inc: Option<&'static Encoding>,

    pub guess_fps_ratio: bool,
    pub no_split_mode: bool,
    pub speed_optimization: Option<f64>,

    pub audio_index: Option<usize>,
}

pub fn parse_args() -> Result<Arguments, InputArgumentsError> {
    let matches = command!()
        .arg(Arg::new("reference-file")
            .help("Path to the reference subtitle or video file")
            .required(true))
        .arg(Arg::new("incorrect-sub-file")
            .help("Path to the incorrect subtitle file. Entering \"_\" here creates debug subtitles, which can later be used as a reference file.")
            .required(true))
        .arg(Arg::new("output-file-path")
            .help("Path to corrected subtitle file")
            .required(true))
        .arg(Arg::new("split-penalty")
            .short('p')
            .long("split-penalty")
            .value_name("floating point number from 0 to 1000")
            .help("Determines how eager the algorithm is to avoid splitting of the subtitles. 1000 means that all lines will be shifted by the same offset, while 0.01 will produce MANY segments with different offsets. Values from 1 to 20 are the most useful.")
            .default_value("7"))
        .arg(Arg::new("interval")
            .short('i')
            .long("interval")
            .value_name("integer in milliseconds")
            .help("The smallest recognized time interval, smaller numbers make the alignment more accurate, greater numbers make aligning faster.")
            .default_value("1"))
        .arg(Arg::new("allow-negative-timestamps")
            .short('n')
            .long("allow-negative-timestamps")
            .help("Negative timestamps can lead to problems with the output file, so by default 0 will be written instead. This option allows you to disable this behavior.")
            .action(ArgAction::SetTrue))
        .arg(Arg::new("sub-fps-ref")
            .long("sub-fps-ref")
            .value_name("floating-point number in frames-per-second")
            .default_value("30")
            .help("Specifies the frames-per-second for the accompanying video of MicroDVD `.sub` files (MicroDVD `.sub` files store timing information as frame numbers). Only affects the reference subtitle file."))
        .arg(Arg::new("sub-fps-inc")
            .long("sub-fps-inc")
            .value_name("floating-point number in frames-per-second")
            .default_value("30")
            .help("Specifies the frames-per-second for the accompanying video of MicroDVD `.sub` files (MicroDVD `.sub` files store timing information as frame numbers). Only affects the incorrect subtitle file."))
        .arg(Arg::new("encoding-ref")
            .long("encoding-ref")
            .value_name("encoding")
            .help("Charset encoding of the reference subtitle file.")
            .default_value("auto"))
        .arg(Arg::new("encoding-inc")
            .long("encoding-inc")
            .value_name("encoding")
            .help("Charset encoding of the incorrect subtitle file.")
            .default_value("auto"))
        .arg(Arg::new("speed-optimization")
            .long("speed-optimization")
            .short('O')
            .value_name("path")
            .default_value("1")
            .help("Greatly speeds up synchronization by sacrificing some accuracy; set to 0 to disable speed optimization")
            .required(false))
        .arg(Arg::new("statistics-required-tag")
            .long("statistics-required-tag")
            .short('t')
            .value_name("tag")
            .help("Only output statistics containing this tag (you can find the tags in statistics file)")
            .required(false))
        .arg(Arg::new("no-split")
            .help("Synchronize subtitles without looking for splits/breaks - this mode is much faster")
            .short('l')
            .long("no-split")
            .action(ArgAction::SetTrue))
        .arg(Arg::new("disable-fps-guessing")
            .help("Disables guessing and correcting of framerate differences between reference file and input file")
            .short('g')
            .long("disable-fps-guessing")
            .alias("disable-framerate-guessing")
            .action(ArgAction::SetTrue))
        .arg(Arg::new("audio-index")
            .help("Specifies the audio index in the reference video file")
            .long("index")
            .value_name("audio-index")
            .required(false))
        .after_help("This program works with .srt, .ass/.ssa, .idx and .sub files. The corrected file will have the same format as the incorrect file.")
        .get_matches();

    let reference_file_path: PathBuf = matches.get_one::<String>("reference-file").unwrap().into();
    let incorrect_file_path: PathBuf = matches.get_one::<String>("incorrect-sub-file").unwrap().into();
    let output_file_path: PathBuf = matches.get_one::<String>("output-file-path").unwrap().into();

    let interval: i64 = unpack_clap_number_i64(&matches, "interval")?;
    if interval < 1 {
        return Err(InputArgumentsErrorKind::ExpectedPositiveNumber {
            argument_name: "interval".to_string(),
            value: interval,
        }
        .into());
    }

    let split_penalty: f64 = unpack_clap_number_f64(&matches, "split-penalty")?;
    let split_penalty_range = 0.0..=1000.0;
    if !split_penalty_range.contains(&split_penalty) {
        return Err(InputArgumentsErrorKind::ValueNotInRange {
            argument_name: "split-penalty".to_string(),
            value: split_penalty,
            min: *split_penalty_range.start(),
            max: *split_penalty_range.end(),
        }
        .into());
    }

    let speed_optimization: f64 = unpack_clap_number_f64(&matches, "speed-optimization")?;
    if speed_optimization < 0.0 {
        return Err(InputArgumentsErrorKind::ExpectedNonNegativeNumber {
            argument_name: "speed-optimization".to_string(),
            value: speed_optimization,
        }
        .into());
    }

    let no_split_mode: bool = matches.get_flag("no-split");

    Ok(Arguments {
        reference_file_path,
        incorrect_file_path,
        output_file_path,
        interval,
        split_penalty,
        sub_fps_ref: unpack_clap_number_f64(&matches, "sub-fps-ref")?,
        sub_fps_inc: unpack_clap_number_f64(&matches, "sub-fps-inc")?,
        allow_negative_timestamps: matches.get_flag("allow-negative-timestamps"),
        encoding_ref: parse_encoding(matches.get_one::<String>("encoding-ref").map(|s| s.as_str())),
        encoding_inc: parse_encoding(matches.get_one::<String>("encoding-inc").map(|s| s.as_str())),
        no_split_mode,
        guess_fps_ratio: !matches.get_flag("disable-fps-guessing"),
        speed_optimization: if speed_optimization <= 0. {
            None
        } else {
            Some(speed_optimization)
        },
        audio_index: unpack_optional_clap_number_usize(&matches, "audio-index")?,
    })
}

/// Reads, parses and does error handling for a f64 clap parameter.
fn unpack_clap_number_f64(
    matches: &clap::ArgMatches,
    parameter_name: &'static str,
) -> Result<f64, InputArgumentsError> {
    let parameter_value_str: &String = matches.get_one(parameter_name).unwrap();
    f64::from_str(parameter_value_str)
        .with_context(|_| InputArgumentsErrorKind::ArgumentParseError {
            argument_name: parameter_name.to_string(),
            value: parameter_value_str.to_string(),
        })
        .map_err(InputArgumentsError::from)
}

/// Reads, parses and does error handling for a i64 clap parameter.
fn unpack_clap_number_i64(
    matches: &clap::ArgMatches,
    parameter_name: &'static str,
) -> Result<i64, InputArgumentsError> {
    let parameter_value_str: &String = matches.get_one(parameter_name).unwrap();
    i64::from_str(parameter_value_str)
        .with_context(|_| InputArgumentsErrorKind::ArgumentParseError {
            argument_name: parameter_name.to_string(),
            value: parameter_value_str.to_string(),
        })
        .map_err(InputArgumentsError::from)
}

fn unpack_optional_clap_number_usize(
    matches: &clap::ArgMatches,
    parameter_name: &'static str,
) -> Result<Option<usize>, InputArgumentsError> {
    match matches.get_one::<String>(parameter_name) {
        None => Ok(None),
        Some(parameter_value_str) => usize::from_str(parameter_value_str)
            .with_context(|_| InputArgumentsErrorKind::ArgumentParseError {
                argument_name: parameter_name.to_string(),
                value: parameter_value_str.to_string(),
            })
            .map(Some)
            .map_err(InputArgumentsError::from),
    }
}

pub fn parse_encoding(opt: Option<&str>) -> Option<&'static Encoding> {
    match opt {
        None | Some("auto") => {
            // use automatic detection
            None
        }
        Some(label) => {
            match Encoding::for_label_no_replacement(label.as_bytes()) {
                None => {
                    // TODO: error handling
                    panic!("{} is not a known encoding label; exiting.", label);
                }
                Some(encoding) => Some(encoding),
            }
        }
    }
}
