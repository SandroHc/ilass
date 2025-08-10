#![allow(unknown_lints)] // for clippy

// TODO: search for unsafe, panic, unimplemented

extern crate clap;
extern crate encoding_rs;
extern crate pbr;
extern crate subparse;

// Alg* stands for algorithm (the internal ilass algorithm types)

use crate::subparse::SubtitleFileInterface;

use clap::{Arg, ArgAction, command};
use encoding_rs::Encoding;
use failure::ResultExt;
use ilass::{TimeDelta as AlgTimeDelta, align};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::result::Result;
use std::str::FromStr;

use subparse::timetypes::*;
use subparse::{SubtitleEntry, SubtitleFormat};

use ilass_cli::errors::*;
use ilass_cli::*;

/// Does reading, parsing and nice error handling for a f64 clap parameter.
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

/// Does reading, parsing and nice error handling for a f64 clap parameter.
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

pub fn get_encoding(opt: Option<&str>) -> Option<&'static Encoding> {
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

// //////////////////////////////////////////////////////////////////////////////////////////////////

struct Arguments {
    reference_file_path: PathBuf,
    incorrect_file_path: PathBuf,
    output_file_path: PathBuf,

    interval: i64,

    split_penalty: f64,

    sub_fps_inc: f64,
    sub_fps_ref: f64,

    allow_negative_timestamps: bool,

    /// having a value of `None` means autodetect encoding
    encoding_ref: Option<&'static Encoding>,
    encoding_inc: Option<&'static Encoding>,

    guess_fps_ratio: bool,
    no_split_mode: bool,
    speed_optimization: Option<f64>,

    audio_index: Option<usize>,
}

fn parse_args() -> Result<Arguments, InputArgumentsError> {
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
        encoding_ref: get_encoding(matches.get_one::<String>("encoding-ref").map(|s| s.as_str())),
        encoding_inc: get_encoding(matches.get_one::<String>("encoding-inc").map(|s| s.as_str())),
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

fn prepare_reference_file(args: &Arguments) -> Result<InputFileHandler, failure::Error> {
    let mut ref_file = InputFileHandler::open(
        &args.reference_file_path,
        args.audio_index,
        args.encoding_ref,
        args.sub_fps_ref,
        ProgressInfo::new(
            500,
            Some(format!(
                "extracting audio from reference file '{}'...",
                args.reference_file_path.display()
            )),
        ),
    )?;

    ref_file.filter_video_with_min_span_length_ms(500);

    Ok(ref_file)
}

// //////////////////////////////////////////////////////////////////////////////////////////////////

fn run() -> Result<(), failure::Error> {
    let args = parse_args()?;

    if args.incorrect_file_path.eq(OsStr::new("_")) {
        // DEBUG MODE FOR REFERENCE FILE WAS ACTIVATED
        let ref_file = prepare_reference_file(&args)?;

        println!("input file path was given as '_'");
        println!("the output file is a .srt file only containing timing information from the reference file");
        println!("this can be used as a debugging tool");
        println!();

        let lines: Vec<(TimeSpan, String)> = ref_file
            .timespans()
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, time_span)| (time_span, format!("line {}", i)))
            .collect();

        let debug_file =
            subparse::SrtFile::create(lines).with_context(|_| TopLevelErrorKind::FailedToInstantiateSubtitleFile)?;

        write_data_to_file(
            &args.output_file_path,
            debug_file.to_data().unwrap(), // error handling
        )?;

        return Ok(());
    }

    // open incorrect file before reference file before so that incorrect-file-not-found-errors are not displayed after the long audio extraction
    let inc_file =
        SubtitleFileHandler::open_sub_file(args.incorrect_file_path.as_path(), args.encoding_inc, args.sub_fps_inc)?;

    let ref_file = prepare_reference_file(&args)?;

    let output_file_format = inc_file.file_format();

    // this program internally stores the files in a non-destructable way (so
    // formatting is preserved) but has no abilty to convert between formats
    if !subparse::is_valid_extension_for_subtitle_format(args.output_file_path.extension(), output_file_format) {
        return Err(TopLevelErrorKind::FileFormatMismatch {
            input_file_path: args.incorrect_file_path,
            output_file_path: args.output_file_path,
            input_file_format: inc_file.file_format(),
        }
        .into_error()
        .into());
    }

    let mut inc_aligner_timespans: Vec<ilass::TimeSpan> = timings_to_alg_timespans(inc_file.timespans(), args.interval);
    let ref_aligner_timespans: Vec<ilass::TimeSpan> = timings_to_alg_timespans(ref_file.timespans(), args.interval);

    let mut fps_scaling_factor = 1.;
    if args.guess_fps_ratio {
        let fps25 = 25.;
        let fps24 = 24.;
        let fps23 = 23.976;
        let ratios = [fps25 / fps24, fps25 / fps23, fps24 / fps25, fps24 / fps23, fps23 / fps25, fps23 / fps24];
        let desc = ["25/24", "25/23.976", "24/25", "24/23.976", "23.976/25", "23.976/24"];

        let (opt_ratio_idx, _) = guess_fps_ratio(
            &ref_aligner_timespans,
            &inc_aligner_timespans,
            &ratios,
            ProgressInfo::new(1, Some("Guessing framerate ratio...".to_string())),
        );

        fps_scaling_factor = if let Some(idx) = opt_ratio_idx { ratios[idx] } else { 1. };

        println!(
            "info: 'reference file FPS/input file FPS' ratio is {}",
            if let Some(idx) = opt_ratio_idx { desc[idx] } else { "1" }
        );
        println!();

        inc_aligner_timespans = inc_aligner_timespans
            .into_iter()
            .map(|x| x.scaled(fps_scaling_factor))
            .collect();
    }

    let align_start_msg = format!(
        "synchronizing '{}' to reference file '{}'...",
        args.incorrect_file_path.display(),
        args.reference_file_path.display()
    );
    let alg_deltas = if args.no_split_mode {
        let num_inc_timespans = inc_aligner_timespans.len();

        let alg_delta = ilass::align_nosplit(
            &ref_aligner_timespans,
            &inc_aligner_timespans,
            ilass::standard_scoring,
            ProgressInfo::new(1, Some(align_start_msg)),
        )
        .0;

        std::vec::from_elem(alg_delta, num_inc_timespans)
    } else {
        align(
            &ref_aligner_timespans,
            &inc_aligner_timespans,
            args.split_penalty,
            args.speed_optimization,
            ilass::standard_scoring,
            ProgressInfo::new(1, Some(align_start_msg)),
        )
        .0
    };
    let deltas = alg_deltas_to_timing_deltas(&alg_deltas, args.interval);

    // group subtitles lines which have the same offset
    let shift_groups: Vec<(AlgTimeDelta, Vec<TimeSpan>)> = get_subtitle_delta_groups(
        alg_deltas
            .iter()
            .cloned()
            .zip(inc_file.timespans().iter().cloned())
            .collect(),
    );

    for (shift_group_delta, shift_group_lines) in shift_groups {
        // computes the first and last timestamp for all lines with that delta
        // -> that way we can provide the user with an information like
        //     "100 subtitles with 10min length"
        let min = shift_group_lines
            .iter()
            .map(|subline| subline.start)
            .min()
            .expect("a subtitle group should have at least one subtitle line");
        let max = shift_group_lines
            .iter()
            .map(|subline| subline.start)
            .max()
            .expect("a subtitle group should have at least one subtitle line");

        println!(
            "shifted block of {} subtitles with length {} by {}",
            shift_group_lines.len(),
            max - min,
            alg_delta_to_delta(shift_group_delta, args.interval)
        );
    }

    println!();

    if ref_file.timespans().is_empty() {
        println!("warn: reference file has no subtitle lines");
        println!();
    }
    if inc_file.timespans().is_empty() {
        println!("warn: file with incorrect subtitles has no lines");
        println!();
    }

    fn scaled_timespan(ts: TimeSpan, fps_scaling_factor: f64) -> TimeSpan {
        TimeSpan::new(
            TimePoint::from_msecs((ts.start.msecs() as f64 * fps_scaling_factor) as i64),
            TimePoint::from_msecs((ts.end.msecs() as f64 * fps_scaling_factor) as i64),
        )
    }

    let mut corrected_timespans: Vec<subparse::timetypes::TimeSpan> = inc_file
        .timespans()
        .iter()
        .zip(deltas.iter())
        .map(|(&timespan, &delta)| scaled_timespan(timespan, fps_scaling_factor) + delta)
        .collect();

    if corrected_timespans.iter().any(|ts| ts.start.is_negative()) {
        println!("warn: some subtitles now have negative timings, which can cause invalid subtitle files");
        if args.allow_negative_timestamps {
            println!(
                "warn: negative timestamps will be written to file, because you passed '-n' or '--allow-negative-timestamps'",
            );
        } else {
            println!(
                "warn: negative subtitles will therefore moved to the start of the subtitle file by default; pass '-n' or '--allow-negative-timestamps' to disable this behavior",
            );

            for corrected_timespan in &mut corrected_timespans {
                if corrected_timespan.start.is_negative() {
                    let offset = TimePoint::from_secs(0) - corrected_timespan.start;
                    corrected_timespan.start += offset;
                    corrected_timespan.end += offset;
                }
            }
        }
        println!();
    }

    // .idx only has start timepoints (the subtitle is shown until the next subtitle starts) - so retiming with gaps might
    // produce errors
    if output_file_format == SubtitleFormat::VobSubIdx {
        println!("warn: writing to an '.idx' file can lead to unexpected results due to restrictions of this format");
    }

    // incorrect file -> correct file
    let shifted_timespans: Vec<SubtitleEntry> = corrected_timespans.into_iter().map(SubtitleEntry::from).collect();

    // write corrected files
    let mut correct_file = inc_file.into_subtitle_file();
    correct_file
        .update_subtitle_entries(&shifted_timespans)
        .with_context(|_| TopLevelErrorKind::FailedToUpdateSubtitle)?;

    write_data_to_file(
        &args.output_file_path,
        correct_file
            .to_data()
            .with_context(|_| TopLevelErrorKind::FailedToGenerateSubtitleData)?,
    )?;

    Ok(())
}

// //////////////////////////////////////////////////////////////////////////////////////////////////

fn main() {
    match run() {
        Ok(_) => std::process::exit(0),
        Err(error) => {
            print_error_chain(error);
            std::process::exit(1)
        }
    }
}
