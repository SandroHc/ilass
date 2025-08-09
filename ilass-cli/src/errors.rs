#![allow(non_local_definitions)]

use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use subparse::SubtitleFormat;
use thiserror::Error;

#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum InputFileError {
    #[error("processing video file '{0}' failed")]
    VideoFile(PathBuf),
    #[error("processing subtitle file '{0}' failed")]
    SubtitleFile(PathBuf),
}

#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum FileOperationError {
    #[error("failed to open file '{path}'")]
    FileOpen { path: PathBuf },
    #[error("failed to read file '{path}'")]
    FileRead { path: PathBuf },
    #[error("failed to write to file '{path}'")]
    FileWrite { path: PathBuf },
}

#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum InputVideoError {
    #[error("failed to extract voice segments from file '{path}'")]
    FailedToDecode { path: PathBuf },
    #[error("failed to analyse audio segment for voice activity")]
    VadAnalysisFailed,
}

#[derive(Debug, Error)]
pub enum InputSubtitleError {
    #[error("reading subtitle file '{0}' failed")]
    ReadingSubtitleFileFailed(PathBuf),
    #[error("unknown subtitle format for file '{0}': {1}")]
    UnknownSubtitleFormat(PathBuf, subparse::errors::Error),
    #[error("parsing subtitle file '{0}' failed: {1}")]
    ParsingSubtitleFailed(PathBuf, subparse::errors::Error),
    #[error("retrieving subtitle file '{0}' failed: {1}")]
    RetrievingSubtitleLinesFailed(PathBuf, subparse::errors::Error),
}

#[derive(Debug, Error)]
pub enum InputArgumentsError {
    #[error("expected value '{argument_name}' to be in range {min}-{max}, found value {value}")]
    ValueNotInRange {
        argument_name: String,
        min: f64,
        max: f64,
        value: f64,
    },
    #[error("expected positive number for '{argument_name}', found {value}")]
    ExpectedPositiveNumber { argument_name: String, value: i64 },

    #[error("expected non-negative number for '{argument_name}', found {value}")]
    ExpectedNonNegativeNumber { argument_name: String, value: f64 },

    #[error("argument '{argument_name}' with value '{value}' could not be parsed")]
    ArgumentParseError { argument_name: String, value: String },
}

#[derive(Clone, PartialEq, Debug, Error)]
pub enum TopLevelError {
    #[error(
        "output file '{output_file_path}' seems to have a different format than input file '{input_file_path}' with format '{input_file_format}' (this program does not perform conversions)"
    )]
    FileFormatMismatch {
        input_file_path: PathBuf,
        output_file_path: PathBuf,
        input_file_format: PrintableSubtitleFormat,
    },
    #[error("failed to change lines in the subtitle")]
    FailedToUpdateSubtitle,
    #[error("failed to generate data for subtitle")]
    FailedToGenerateSubtitleData,
    #[error("failed to instantiate subtitle file")]
    FailedToInstantiateSubtitleFile,
}

#[derive(Clone, PartialEq, Debug)]
pub struct PrintableSubtitleFormat(pub SubtitleFormat);

impl Display for PrintableSubtitleFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.get_name())
    }
}
