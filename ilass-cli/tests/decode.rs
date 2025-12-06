use ilass_cli::{InputFileHandler, NoProgressInfo};
use serde::Serializer;
use serde::ser::SerializeStruct;
use std::path::PathBuf;
use subparse::timetypes::TimeSpan;

#[test]
fn subtitle() {
    insta::assert_yaml_snapshot!(read_timespans("oppenheimer.ref.srt"));
}

#[test]
fn audio() {
    insta::assert_yaml_snapshot!(read_timespans("oppenheimer.aac"));
}

#[test]
fn video() {
    insta::assert_yaml_snapshot!(read_timespans("oppenheimer.mp4"));
}

fn read_timespans(file_name: &str) -> Vec<SerializableTimeSpan> {
    let file_path = samples_dir().join(file_name);
    let file = InputFileHandler::open(&file_path, None, None, 0.0, NoProgressInfo {})
        .unwrap_or_else(|_| panic!("invalid sample file: {}", file_path.display()));

    file.timespans()
        .iter()
        .copied()
        .map(SerializableTimeSpan::from)
        .collect::<Vec<_>>()
}

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("samples")
}

struct SerializableTimeSpan(TimeSpan);

impl From<TimeSpan> for SerializableTimeSpan {
    fn from(value: TimeSpan) -> Self {
        SerializableTimeSpan(value)
    }
}

impl serde::Serialize for SerializableTimeSpan {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_struct("TimeSpan", 2)?;
        map.serialize_field("start", self.0.start.to_string().as_str())?;
        map.serialize_field("end", self.0.end.to_string().as_str())?;
        map.end()
    }
}
