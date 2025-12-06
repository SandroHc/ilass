// Alg* stands for algorithm (the internal ilass algorithm types)

use failure::ResultExt;
use ilass::{TimeDelta as AlgTimeDelta, align};
use std::ffi::OsStr;
use std::result::Result;
use subparse::timetypes::{TimePoint, TimeSpan as SubTimeSpan};
use subparse::{SubtitleEntry, SubtitleFileInterface, SubtitleFormat};

use ilass_cli::args::Arguments;
use ilass_cli::errors::TopLevelErrorKind;
use ilass_cli::*;

fn main() {
    match run() {
        Ok(_) => std::process::exit(0),
        Err(error) => {
            print_error_chain(error);
            std::process::exit(1)
        }
    }
}

fn run() -> Result<(), failure::Error> {
    let args = args::parse_args()?;

    if args.incorrect_file_path.eq(OsStr::new("_")) {
        // DEBUG MODE FOR REFERENCE FILE WAS ACTIVATED
        let ref_file = prepare_reference_file(&args)?;

        println!("input file path was given as '_'");
        println!("the output file is a .srt file only containing timing information from the reference file");
        println!("this can be used as a debugging tool");
        println!();

        let lines: Vec<(SubTimeSpan, String)> = ref_file
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
        let a = 25.;
        let b = 24.;
        let c = 23.976;
        let ratios = [a / b, a / c, b / a, b / c, c / a, c / b];
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
    let shift_groups: Vec<(AlgTimeDelta, Vec<SubTimeSpan>)> = get_subtitle_delta_groups(
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

    fn scaled_timespan(ts: SubTimeSpan, fps_scaling_factor: f64) -> SubTimeSpan {
        SubTimeSpan::new(
            TimePoint::from_msecs((ts.start.msecs() as f64 * fps_scaling_factor) as i64),
            TimePoint::from_msecs((ts.end.msecs() as f64 * fps_scaling_factor) as i64),
        )
    }

    let mut corrected_timespans: Vec<SubTimeSpan> = inc_file
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
