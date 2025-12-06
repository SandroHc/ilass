use ilass_cli::{
    InputFileHandler, NoProgressInfo, SubtitleFileHandler, alg_deltas_to_timing_deltas, timings_to_alg_timespans,
};
use std::path::PathBuf;
use subparse::SubtitleEntry;

#[test]
fn oppenheimer() {
    insta::assert_snapshot!("oppenheimer_srt", align("oppenheimer.in.srt", "oppenheimer.ref.srt"));
    insta::assert_snapshot!("oppenheimer_aac", align("oppenheimer.in.srt", "oppenheimer.aac"));
}

#[test]
fn ref_eq_in() {
    // There should be absolutely no difference.
    insta::assert_snapshot!(align("oppenheimer.ref.srt", "oppenheimer.ref.srt"));
}

/// Aligns the subtitles in `in_path` using `ref_path` as reference (which can be a subtitle, audio or video file).
///
/// The output is the contents of a subtitle file in the same format as `in_path`.
fn align(in_path: &str, ref_path: &str) -> String {
    const INTERVAL: i64 = 1;
    const SPEED_OPTIMIZATION_DISABLE: Option<f64> = Some(0.0);
    const SPLIT_PENALTY: f64 = 7.0;

    let in_path = samples_dir().join(in_path);
    let in_file = SubtitleFileHandler::open_sub_file(&in_path, None, 0.0)
        .unwrap_or_else(|_| panic!("invalid input file: {}", in_path.display()));

    let ref_path = samples_dir().join(ref_path);
    let ref_file = InputFileHandler::open(&ref_path, None, None, 0.0, NoProgressInfo {})
        .unwrap_or_else(|_| panic!("invalid reference file: {}", ref_path.display()));

    let in_timespans = timings_to_alg_timespans(in_file.timespans(), INTERVAL);
    let ref_timespans = timings_to_alg_timespans(ref_file.timespans(), INTERVAL);

    // Calculate deltas between reference and input timespans
    let deltas = ilass::align(
        &ref_timespans,
        &in_timespans,
        SPLIT_PENALTY,
        SPEED_OPTIMIZATION_DISABLE,
        ilass::standard_scoring,
        NoProgressInfo {},
    )
    .0;
    let deltas = alg_deltas_to_timing_deltas(&deltas, INTERVAL);

    // Align subtitles with new timespans
    let corrected_timespans = in_file
        .timespans()
        .iter()
        .zip(deltas.iter())
        .map(|(&ts, &delta)| ts + delta)
        .map(SubtitleEntry::from)
        .collect::<Vec<_>>();

    let mut corrected_file = in_file.into_subtitle_file();
    corrected_file
        .update_subtitle_entries(&corrected_timespans)
        .expect("failed to shift subtitles");

    // Serialize to file
    let encoded = corrected_file.to_data().expect("failed to serialize subtitles");
    String::from_utf8(encoded).expect("serialized subtitles are not valid UTF-8")
}

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("samples")
}
