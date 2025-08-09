#![allow(non_local_definitions)]

use std::ffi::OsString;
use std::fmt;
use std::fmt::{Debug, Display, Formatter, Write};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::process::{ChildStdout, Command, Output, Stdio};
use std::str::from_utf8;

use byteorder::ByteOrder;
use serde::{Deserialize, Deserializer};
use thiserror::Error;

#[derive(Debug, PartialEq, Eq)]
pub enum CodecType {
    Audio,
    Video,
    Subtitle,
    Other(String),
}

impl<'de> Deserialize<'de> for CodecType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "audio" => Ok(CodecType::Audio),
            "video" => Ok(CodecType::Video),
            "subtitle" => Ok(CodecType::Subtitle),
            s => Ok(CodecType::Other(s.to_owned())),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Stream {
    pub index: usize,
    pub channels: Option<usize>,
    /// `.mkv` does not store the duration in the streams; we have to use `format -> duration` instead
    pub duration: Option<String>,
    pub codec_type: CodecType,
}

#[derive(Debug, Deserialize)]
struct Format {
    pub duration: Option<String>,
}

/// Metadata associated with a video.
#[derive(Debug, Deserialize)]
struct Metadata {
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Debug, Error)]
pub enum DecoderError {
    #[error("failed to decode video stream info")]
    FailedToDecodeVideoStreamInfo,
    #[error("failed to extract metadata from '{file_path}' using command '{cmd}'")]
    ExtractingMetadataFailed { file_path: PathBuf, cmd: CmdWithArgs },
    #[error("no audio stream in file '{path}'")]
    NoAudioStream { path: PathBuf },
    #[error("failed to extract audio from '{file_path}' with '{cmd}'")]
    FailedExtractingAudio { file_path: PathBuf, cmd: CmdWithArgs },
    #[error("failed to spawn subprocess '{cmd}'")]
    FailedSpawningSubprocess { cmd: CmdWithArgs },
    #[error("failed to check status of subprocess '{cmd_path}'")]
    WaitingForProcessFailed { cmd_path: PathBuf },
    #[error("process '{cmd_path}' returned error code '{code}'")]
    ProcessErrorCode { cmd_path: PathBuf, code: ProcessErrorCode },
    #[error("stderr: {msg}")]
    ProcessErrorMessage { msg: String },
    #[error("failed to deserialize metadata of file '{path}'")]
    DeserializingMetadataFailed { path: PathBuf },
    #[error("error while reading stdout")]
    ReadError,
    #[error("failed to parse duration string '{s}' from metadata")]
    FailedToParseDuration { s: String },
    #[error("processing audio segment failed")]
    AudioSegmentProcessingFailed,
    #[error("no audio duration information found")]
    NoDurationInformation,
}

#[derive(Debug)]
pub struct CmdWithArgs {
    pub cmd_path: PathBuf,
    pub args: Vec<OsString>,
}

impl Display for CmdWithArgs {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.cmd_path.display())?;

        let mut first = true;
        for arg in &self.args {
            if first {
                first = false;
            } else {
                f.write_char(' ')?;
            }
            write!(f, "{}", arg.to_string_lossy())?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct ProcessErrorCode(Option<i32>);

impl Display for ProcessErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let code_str = self
            .0
            .map(|x| x.to_string())
            .unwrap_or_else(|| String::from("interrupted?"));
        f.write_str(&code_str)
    }
}

trait IntoOk<T> {
    fn into_ok<I>(self) -> Result<T, I>;
}
impl<T> IntoOk<T> for T {
    fn into_ok<I>(self) -> Result<T, I> {
        Ok(self)
    }
}

pub struct VideoDecoderFFmpegBinary {}

static PROGRESS_PRESCALER: i64 = 200;

impl VideoDecoderFFmpegBinary {
    /// Samples are pushed in 8kHz mono/single-channel format.
    pub fn decode<T>(
        file_path: impl AsRef<Path>,
        audio_index: Option<usize>,
        receiver: impl super::AudioReceiver<Output = T>,
        mut progress_handler: impl super::ProgressHandler,
    ) -> color_eyre::Result<T, DecoderError> {
        let file_path_buf: PathBuf = file_path.as_ref().into();

        let args = vec![
            OsString::from("-v"),
            OsString::from("error"),
            OsString::from("-show_entries"),
            OsString::from("format=duration:stream=index,channels,duration,codec_type"),
            OsString::from("-of"),
            OsString::from("json"),
            OsString::from(file_path.as_ref()),
        ];

        let ffprobe_path: PathBuf = std::env::var_os("ILASS_FFPROBE_PATH")
            .unwrap_or(OsString::from("ffprobe"))
            .into();

        let metadata: Metadata =
            Self::get_metadata(file_path_buf.clone(), ffprobe_path.clone(), &args).map_err(|_| {
                DecoderError::ExtractingMetadataFailed {
                    file_path: file_path_buf.clone(),
                    cmd: CmdWithArgs {
                        cmd_path: ffprobe_path.clone(),
                        args,
                    },
                }
            })?;

        let mut audio_streams = metadata
            .streams
            .into_iter()
            .filter(|s| s.codec_type == CodecType::Audio && s.channels.is_some());

        let best_stream_opt = match audio_index {
            None => audio_streams.min_by_key(|s| s.channels.unwrap()),
            Some(ai) => audio_streams.find(|s| s.index == ai),
        };

        let best_stream: Stream = match best_stream_opt {
            Some(x) => x,
            None => {
                return Err(DecoderError::NoAudioStream {
                    path: file_path.as_ref().into(),
                });
            }
        };

        let ffmpeg_path: PathBuf = std::env::var_os("ILASS_FFMPEG_PATH")
            .unwrap_or(OsString::from("ffmpeg"))
            .into();

        let args: Vec<OsString> = vec![
            // only print errors
            OsString::from("-v"),
            OsString::from("error"),
            // "yes" -> disables user interaction
            OsString::from("-y"),
            // input file
            OsString::from("-i"),
            file_path.as_ref().into(),
            // select stream
            OsString::from("-map"),
            format!("0:{}", best_stream.index).into(),
            // audio codec: 16-bit signed little endian
            OsString::from("-acodec"),
            OsString::from("pcm_s16le"),
            // resample to 8khz
            OsString::from("-ar"),
            OsString::from("8000"),
            // resample to single channel
            OsString::from("-ac"),
            OsString::from("1"),
            // output 16-bit signed little endian stream directly (no wav, etc.)
            OsString::from("-f"),
            OsString::from("s16le"),
            // output to stdout pipe
            OsString::from("-"),
        ];

        let format_opt: Option<Format> = metadata.format;

        // `.mkv` containers do not store duration info in streams, only the format information does contain it
        let duration_str = best_stream
            .duration
            .or_else(|| format_opt.and_then(|format| format.duration))
            .ok_or(DecoderError::NoDurationInformation)?;

        let duration = duration_str
            .parse::<f64>()
            .map_err(|_| DecoderError::FailedToParseDuration { s: duration_str })?;

        let num_samples: i64 = (duration * 8000.0) as i64 / PROGRESS_PRESCALER;

        progress_handler.init(num_samples);

        Self::extract_audio_stream(receiver, progress_handler, ffmpeg_path.clone(), &args)
            .map_err(|_| DecoderError::FailedExtractingAudio {
                file_path: file_path_buf.clone(),
                cmd: CmdWithArgs {
                    cmd_path: ffmpeg_path.clone(),
                    args,
                },
            })?
            .into_ok()
    }

    fn extract_audio_stream<T>(
        mut receiver: impl super::AudioReceiver<Output = T>,
        mut progress_handler: impl super::ProgressHandler,
        ffmpeg_path: PathBuf,
        args: &[OsString],
    ) -> color_eyre::Result<T, DecoderError> {
        let mut ffmpeg_process: Child = Command::new(ffmpeg_path.clone())
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|_| DecoderError::FailedSpawningSubprocess {
                cmd: CmdWithArgs {
                    cmd_path: ffmpeg_path.clone(),
                    args: args.to_vec(),
                },
            })?;

        let mut stdout: ChildStdout = ffmpeg_process.stdout.take().unwrap();

        enum ParserState {
            Start,
            SingleByte(u8),
        }

        let mut data: Vec<u8> = std::vec::from_elem(0, 200 * 1024 * 1024);
        let data2_cap = 1024 * 1024;
        let mut data2: Vec<i16> = Vec::with_capacity(data2_cap);
        let mut parser_state: ParserState = ParserState::Start;
        let mut progress_prescaler_counter = 0;

        loop {
            // improves performance by allowing ffmpeg to generate more data in pipe
            // TODO: an async tokio read might also have the same effect (without being as machine dependent)
            //  -> too low: does not do anything (+some otherhead)
            //  -> too high: slows down computaton because ffmpeg has to wait for this process to read
            //std::thread::sleep(Duration::from_nanos(1000));

            let read_bytes = stdout.read(&mut data).map_err(|_| DecoderError::ReadError)?;
            //println!("{}", read_bytes);

            if read_bytes == 0 {
                match ffmpeg_process
                    .wait()
                    .map_err(|_| DecoderError::WaitingForProcessFailed {
                        cmd_path: ffmpeg_path.clone(),
                    })?
                    .code()
                {
                    Some(0) => {
                        receiver
                            .push_samples(&data2)
                            .map_err(|_| DecoderError::AudioSegmentProcessingFailed)?;
                        data2.clear();
                        progress_handler.finish();
                        return receiver
                            .finish()
                            .map_err(|_| DecoderError::AudioSegmentProcessingFailed);
                    }
                    code @ Some(_) | code @ None => {
                        let mut stderr_data = Vec::new();
                        ffmpeg_process
                            .stderr
                            .unwrap()
                            .read_to_end(&mut stderr_data)
                            .map_err(|_| DecoderError::ReadError)?;

                        let stderr_str: String = String::from_utf8_lossy(&stderr_data).into();

                        return if stderr_str.is_empty() {
                            Err(DecoderError::ProcessErrorCode {
                                cmd_path: ffmpeg_path,
                                code: ProcessErrorCode(code),
                            })
                        } else {
                            Err(DecoderError::ProcessErrorMessage { msg: stderr_str })
                        };
                    }
                }
            }

            for &byte in &data[0..read_bytes] {
                match parser_state {
                    ParserState::Start => parser_state = ParserState::SingleByte(byte),
                    ParserState::SingleByte(last_byte) => {
                        let two_bytes = [last_byte, byte];
                        let sample = byteorder::LittleEndian::read_i16(&two_bytes);
                        receiver
                            .push_samples(&[sample])
                            .map_err(|_| DecoderError::AudioSegmentProcessingFailed)?;

                        if progress_prescaler_counter == PROGRESS_PRESCALER {
                            progress_handler.inc();
                            progress_prescaler_counter = 0;
                        }

                        progress_prescaler_counter += 1;

                        /*data2.push(sample);
                        if data2.len() == data2_cap {
                            receiver.push_samples(&data2);
                            data2.clear();
                        }*/
                        parser_state = ParserState::Start;
                    }
                }
            }
        }
    }

    fn get_metadata(
        file_path: PathBuf,
        ffprobe_path: PathBuf,
        args: &[OsString],
    ) -> color_eyre::Result<Metadata, DecoderError> {
        let ffprobe_process: Output = Command::new(ffprobe_path.clone())
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .output()
            .map_err(|_| DecoderError::FailedSpawningSubprocess {
                cmd: CmdWithArgs {
                    cmd_path: ffprobe_path.clone(),
                    args: args.to_vec(),
                },
            })?;

        if !ffprobe_process.status.success() {
            let stderr: String = String::from_utf8_lossy(&ffprobe_process.stderr)
                .to_string()
                .trim_end()
                .to_string();

            return if stderr.is_empty() {
                Err(DecoderError::ProcessErrorCode {
                    cmd_path: ffprobe_path.clone(),
                    code: ProcessErrorCode(ffprobe_process.status.code()),
                })
            } else {
                Err(DecoderError::ProcessErrorMessage { msg: stderr })
            };
        }

        let stdout = from_utf8(&ffprobe_process.stdout).map_err(|_| DecoderError::FailedToDecodeVideoStreamInfo)?;

        let metadata: Metadata =
            serde_json::from_str(stdout).map_err(|_| DecoderError::DeserializingMetadataFailed { path: file_path })?;

        Ok(metadata)
    }
}
