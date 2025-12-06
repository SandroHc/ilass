The integration tests in this directory use a technique called "test snapshotting" via [cargo-insta](https://insta.rs/), where new test executions generate a snapshot of the state and compare it with a known good version. Snapshots are saved in the sub-directory "snapshots".

These tests can be run like normal tests via `cargo test` or via `cargo insta test`. Any discrepancies with existing snapshots have to be reviewed with `cargo insta review`.

## Conversions

In the sub-directory "samples" lie all the reference files for the integration tests. These files include source (or reference) videos, audios and subtitles, and known bad subtitles that need to be synced.

We are interested in testing support for different subtitle formats (.srt, .ass, etc.), audio formats (.mp3, .acc, etc.) and popular media containers (.mp4, .mkv, etc.). Super high-fidelity source files are not necessary, so it is suggested to remove or downsample video streams to reduce disk usage.

For a perceptually lossless re-encoding to AAC[^ffmpeg-aac], execute:

    ffmpeg -i in.mp4 -map 0:a:0 -b:a 64k -ar 24000 out.aac


[^ffmpeg-aac]: https://trac.ffmpeg.org/wiki/Encode/AAC



----


# Integration Tests

This directory contains integration tests that ensures there are no regressions in the alignment of out-of-sync subtitles by comparing them against reference subtitles or audio tracks.

## Methodology

Tests use snapshot testing via [cargo-insta](https://insta.rs/). Each test execution generates a snapshot of the synchronized output and compares it against a known good version stored in the `snapshots/` subdirectory.

**Running tests:**
- Standard execution: `cargo test`, or `cargo insta test`
- Review changes: `cargo insta review`

Any differences from existing snapshots must be manually reviewed and approved.

## Samples

The `samples/` subdirectory contains reference files for testing:

- **Source media**: Reference videos and audio files with correct timing
- **Reference subtitles**: Properly synchronized subtitle files
- **Out-of-sync subtitles**: Test cases requiring synchronization

### Format Coverage

Tests validate synchronization across:
- Subtitle formats: `.srt`, `.ass`, and others
- Audio formats: `.mp3`, `.aac`, and others
- Media containers: `.mp4`, `.mkv`, and others

### Sample Preparation

High-fidelity source files aren't required. To minimize disk usage, downsample or remove video streams from test files.

**Example - Convert to perceptually lossless AAC[^1]:**

```bash
ffmpeg -i in.mp4 -map 0:a:0 -b:a 64k -ar 24000 out.aac
```

[^1]: https://trac.ffmpeg.org/wiki/Encode/AAC
