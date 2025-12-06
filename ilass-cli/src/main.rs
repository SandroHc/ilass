// Alg* stands for algorithm (the internal ilass algorithm types)

use failure::Error;
use failure::ResultExt;
use ilass::{TimeDelta as AlgTimeDelta, TimeDelta, TimeSpan};
use std::ffi::OsStr;
use std::path::Path;
use std::result::Result;
use subparse::timetypes::{TimeDelta as SubTimeDelta, TimePoint as SubTimePoint, TimePoint, TimeSpan as SubTimeSpan};
use subparse::{SubtitleEntry, SubtitleFileInterface};

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

fn run() -> Result<(), Error> {
    let args = args::parse_args()?;
    debug_mode(&args)?;

    // Open the incorrect file before the reference file so that incorrect-file-not-found-errors are
    // displayed before the long audio extraction.
    let in_file =
        SubtitleFileHandler::open_sub_file(args.incorrect_file_path.as_path(), args.encoding_inc, args.sub_fps_inc)?;
    if in_file.timespans().is_empty() {
        println!("warn: file with incorrect subtitles has no lines");
        println!();
    }

    // We do not do any modifications to the files we read, so formatting is preserved.
    // This prevents us from converting between formats.
    if !subparse::is_valid_extension_for_subtitle_format(args.output_file_path.extension(), in_file.file_format()) {
        return Err(TopLevelErrorKind::FileFormatMismatch {
            input_file_path: args.incorrect_file_path,
            output_file_path: args.output_file_path,
            input_file_format: in_file.file_format(),
        }
        .into_error()
        .into());
    }

    let ref_file = prepare_reference_file(&args)?;
    if ref_file.timespans().is_empty() {
        println!("warn: reference file has no subtitle lines");
        println!();
    }

    let mut in_timespans = timings_to_alg_timespans(in_file.timespans(), args.interval);
    let ref_timespans = timings_to_alg_timespans(ref_file.timespans(), args.interval);

    let fps_scaling_factor = guess_fps_scaling_factor(&args, &mut in_timespans, &ref_timespans);

    let align_start_msg = format!(
        "synchronizing '{}' to reference file '{}'...",
        args.incorrect_file_path.display(),
        args.reference_file_path.display()
    );
    let alg_deltas = if args.no_split_mode {
        let alg_delta = ilass::align_nosplit(
            &ref_timespans,
            &in_timespans,
            ilass::standard_scoring,
            ProgressInfo::new(1, Some(align_start_msg)),
        )
        .0;

        std::vec::from_elem(alg_delta, in_timespans.len())
    } else {
        ilass::align(
            &ref_timespans,
            &in_timespans,
            args.split_penalty,
            args.speed_optimization,
            ilass::standard_scoring,
            ProgressInfo::new(1, Some(align_start_msg)),
        )
        .0
    };
    let deltas = alg_deltas_to_timing_deltas(&alg_deltas, args.interval);
    let corrected_timespans = correct_timespans(&in_file, &deltas, fps_scaling_factor, args.allow_negative_timestamps)?;

    print_changes(&in_file, &alg_deltas, args.interval);

    let corrected_file = in_file.into_subtitle_file();
    write_results(&args.output_file_path, corrected_file, &corrected_timespans)?;

    Ok(())
}

fn debug_mode(args: &Arguments) -> Result<(), Error> {
    if args.incorrect_file_path.eq(OsStr::new("_")) {
        // DEBUG MODE FOR REFERENCE FILE WAS ACTIVATED
        let ref_file = prepare_reference_file(args)?;

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

        std::process::exit(0);
    }

    Ok(())
}

fn guess_fps_scaling_factor(
    args: &Arguments,
    in_aligner_timespans: &mut [TimeSpan],
    ref_aligner_timespans: &[TimeSpan],
) -> f64 {
    const DEFAULT_SCALING_FACTOR: f64 = 1.0;

    if args.guess_fps_ratio {
        const FPS25: f64 = 25.;
        const FPS24: f64 = 24.;
        const FPS23: f64 = 23.976;
        const RATIOS: [f64; 6] = [
            FPS25 / FPS24,
            FPS25 / FPS23,
            FPS24 / FPS25,
            FPS24 / FPS23,
            FPS23 / FPS25,
            FPS23 / FPS24,
        ];
        const DESC: [&str; 6] = ["25/24", "25/23.976", "24/25", "24/23.976", "23.976/25", "23.976/24"];

        let (opt_ratio_idx, _) = guess_fps_ratio(
            ref_aligner_timespans,
            in_aligner_timespans,
            &RATIOS,
            ProgressInfo::new(1, Some("Guessing framerate ratio...".to_string())),
        );

        let fps_scaling_factor = opt_ratio_idx.map(|idx| RATIOS[idx]).unwrap_or(DEFAULT_SCALING_FACTOR);

        println!(
            "info: 'reference file FPS/input file FPS' ratio is {}",
            if let Some(idx) = opt_ratio_idx { DESC[idx] } else { "1" }
        );
        println!();

        for ts in in_aligner_timespans {
            *ts = ts.scaled(fps_scaling_factor);
        }

        fps_scaling_factor
    } else {
        DEFAULT_SCALING_FACTOR
    }
}

fn print_changes(inc_file: &SubtitleFileHandler, alg_deltas: &[TimeDelta], interval: i64) {
    // Group subtitles lines with the same offset
    let shift_groups: Vec<(AlgTimeDelta, Vec<SubTimeSpan>)> = get_subtitle_delta_groups(
        alg_deltas
            .iter()
            .cloned()
            .zip(inc_file.timespans().iter().cloned())
            .collect(),
    );

    for (shift_group_delta, shift_group_lines) in shift_groups {
        // Computes the first and last timestamp for all lines with that delta. With this
        // information, we show information like "100 subtitles with 10 min. length" to the user.
        let min = shift_group_lines
            .iter()
            .map(|subline| subline.start)
            .min()
            .unwrap_or(TimePoint::from_secs(0));
        let max = shift_group_lines
            .iter()
            .map(|subline| subline.start)
            .max()
            .unwrap_or(TimePoint::from_secs(0));

        println!(
            "shifted block of {} subtitles with length {} by {}",
            shift_group_lines.len(),
            max - min,
            alg_delta_to_delta(shift_group_delta, interval)
        );
    }

    println!();
}

fn correct_timespans(
    in_file: &SubtitleFileHandler,
    deltas: &[SubTimeDelta],
    fps_scaling_factor: f64,
    allow_negative_timestamps: bool,
) -> Result<Vec<SubTimeSpan>, Error> {
    fn scaled_timespan(ts: SubTimeSpan, fps_scaling_factor: f64) -> SubTimeSpan {
        SubTimeSpan::new(
            SubTimePoint::from_msecs((ts.start.msecs() as f64 * fps_scaling_factor) as i64),
            SubTimePoint::from_msecs((ts.end.msecs() as f64 * fps_scaling_factor) as i64),
        )
    }

    let mut corrected_timespans: Vec<SubTimeSpan> = in_file
        .timespans()
        .iter()
        .zip(deltas.iter())
        .map(|(&timespan, &delta)| scaled_timespan(timespan, fps_scaling_factor) + delta)
        .collect();

    if corrected_timespans.iter().any(|ts| ts.start.is_negative()) {
        println!("warn: some subtitles now have negative timings, which can cause invalid subtitle files");
        if allow_negative_timestamps {
            println!(
                "warn: negative timestamps will be written to file, because you passed '-n' or '--allow-negative-timestamps'",
            );
        } else {
            println!(
                "warn: negative subtitles will therefore moved to the start of the subtitle file by default; pass '-n' or '--allow-negative-timestamps' to disable this behavior",
            );

            for corrected_timespan in &mut corrected_timespans {
                if corrected_timespan.start.is_negative() {
                    let offset = SubTimePoint::from_secs(0) - corrected_timespan.start;
                    corrected_timespan.start += offset;
                    corrected_timespan.end += offset;
                }
            }
        }
        println!();
    }

    Ok(corrected_timespans)
}

fn write_results(
    output_file_path: &Path,
    mut out_file: subparse::SubtitleFile,
    corrected_timespans: &[SubTimeSpan],
) -> Result<(), Error> {
    // The format .idx does not have end timepoints (the subtitle is shown until the next subtitle
    // starts), so retiming with gaps might produce errors
    if matches!(out_file, subparse::SubtitleFile::VobSubIdxFile(_)) {
        println!("warn: writing to an '.idx' file can lead to unexpected results due to restrictions of this format");
    }

    // Align subtitles with new timespans
    let shifted_timespans: Vec<SubtitleEntry> = corrected_timespans.iter().copied().map(SubtitleEntry::from).collect();
    out_file
        .update_subtitle_entries(&shifted_timespans)
        .with_context(|_| TopLevelErrorKind::FailedToUpdateSubtitle)?;

    write_data_to_file(
        output_file_path,
        out_file
            .to_data()
            .with_context(|_| TopLevelErrorKind::FailedToGenerateSubtitleData)?,
    )?;

    Ok(())
}

fn prepare_reference_file(args: &Arguments) -> Result<InputFileHandler, Error> {
    // Ignore subtitles that are shorter than 500 ms
    const MIN_SPAN_LEN_MS: i64 = 500;

    let mut ref_file = InputFileHandler::open(
        &args.reference_file_path,
        args.audio_index,
        args.encoding_ref,
        args.sub_fps_ref,
        ProgressInfo::new(
            MIN_SPAN_LEN_MS,
            Some(format!(
                "extracting audio from reference file '{}'...",
                args.reference_file_path.display()
            )),
        ),
    )?;
    ref_file.filter_video_with_min_span_length_ms(MIN_SPAN_LEN_MS);

    Ok(ref_file)
}
