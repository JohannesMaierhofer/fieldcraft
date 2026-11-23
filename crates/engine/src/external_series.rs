//! Playback of measured or computed nodal fields on a fixed mesh.
//! All frames are validated before the session changes. Playback only reads memory.
use crate::cmd::{file, forms, opt};
use crate::external::{ExternalField, deformation_scale};
use crate::external_catalog::{Kind, Selection, sibling};
use crate::requirement::Requirement;
use crate::{CommandKind, CommandSpec, EngineError, Result, Session};
use fieldcraft_render::nodal::FieldFile;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

pub(crate) const SERIES: CommandSpec = CommandSpec {
    id: "results.external.series",
    label: "Open External Field Series",
    params: r#"{"path":"B.series.json","component"?: "auto|magnitude|x|y|z","deformation_scale"?: 0} — open a mesh-bound time series (v1) or a parameter sweep (v2) on its first frame. All frames use one colour scale. A positive deformation scale displays nodal displacement vectors in metres. Use {"entry":"name"} instead of path to reopen a checked catalog row. Use results.external.playback to seek or play."#,
    form: Some(forms::EXTERNAL_SERIES),
    kind: CommandKind::Session,
    requires: &[Requirement::Mesh],
    run: load,
};
pub(crate) const PLAYBACK: CommandSpec = CommandSpec {
    id: "results.external.playback",
    label: "External Field Playback",
    params: r#"{"frame"?: 0,"playing"?: true} — select a zero-based frame and pause, or play across the recorded axis in four display seconds. Starting at the last frame replays from the first. With no parameters, report the current frame and its time in seconds (v1) or named parameter value and unit (v2)."#,
    form: Some(forms::EXTERNAL_PLAYBACK),
    kind: CommandKind::Session,
    requires: &[Requirement::ExternalSeries],
    run: control,
};

pub(crate) const MAX_FRAMES: usize = 4096;
// 96 MiB of retained samples: each label and display value takes 12 bytes, plus 24 for a displacement vector.
const MAX_SAMPLES: usize = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const PLAY_SECONDS: f64 = 4.;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeriesFile {
    format: String,
    mesh_sha256: String,
    name: String,
    unit: String,
    components: usize,
    #[serde(default, deserialize_with = "present")]
    time_unit: Option<String>,
    #[serde(default, deserialize_with = "present")]
    axis: Option<ParameterAxis>,
    frames: Vec<Entry>,
}

// A missing member selects the other format. An explicit null is invalid in either format.
fn present<'de, T: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Entry {
    Time { time: f64, file: String },
    Parameter { parameter: f64, file: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParameterAxis {
    label: String,
    unit: String,
}

#[derive(Clone, Debug)]
enum Axis {
    Time,
    Parameter(ParameterAxis),
}

impl Axis {
    fn read(format: &str, time_unit: Option<&str>, axis: Option<ParameterAxis>) -> Result<Self> {
        match (format, time_unit, axis) {
            ("fieldcraft-nodal-series/1", Some("s"), None) => Ok(Self::Time),
            ("fieldcraft-nodal-series/2", None, Some(axis)) => {
                if axis.label.trim().is_empty()
                    || axis.label.len() > 120
                    || axis.unit.len() > 48
                    || axis.label.chars().chain(axis.unit.chars()).any(char::is_control)
                {
                    return Err(error("invalid series axis label or unit"));
                }
                Ok(Self::Parameter(axis))
            }
            _ => Err(error("expected a fieldcraft-nodal-series/1 in seconds or a fieldcraft-nodal-series/2 with a parameter axis")),
        }
    }

    fn sample<'a>(&self, entry: &'a Entry) -> Result<(f64, &'a str)> {
        match (self, entry) {
            (Self::Time, Entry::Time { time, file }) => Ok((*time, file)),
            (Self::Parameter(_), Entry::Parameter { parameter, file }) => Ok((*parameter, file)),
            _ => Err(error("series/1 frames need time; series/2 frames need parameter")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Series {
    frames: Arc<Vec<(f64, ExternalField)>>,
    axis: Axis,
    revision: u64,
    index: usize,
    position: f64,
    playing: bool,
}

impl Series {
    pub fn frame(&self) -> usize {
        self.index
    }
    pub fn count(&self) -> usize {
        self.frames.len()
    }
    pub fn playing(&self) -> bool {
        self.playing
    }
    /// Legacy accessor for timed series. Use `frame_value` and the axis metadata for either format.
    pub fn frame_time(&self) -> f64 {
        self.frame_value()
    }
    /// Physical coordinate of the displayed frame.
    pub fn frame_value(&self) -> f64 {
        self.frames.get(self.index).map_or(self.position, |f| f.0)
    }
    pub fn is_time(&self) -> bool {
        matches!(self.axis, Axis::Time)
    }
    pub fn axis_label(&self) -> &str {
        match &self.axis {
            Axis::Time => "Time",
            Axis::Parameter(axis) => &axis.label,
        }
    }
    pub fn axis_unit(&self) -> &str {
        match &self.axis {
            Axis::Time => "s",
            Axis::Parameter(axis) => &axis.unit,
        }
    }
    fn with_position(&self, mut reply: Value) -> Value {
        match &self.axis {
            Axis::Time => {
                reply["time"] = json!(self.frame_value());
                reply["time_unit"] = json!("s");
            }
            Axis::Parameter(axis) => {
                reply["parameter"] = json!(self.frame_value());
                reply["axis"] = json!(axis);
            }
        }
        reply
    }
    pub(crate) fn field(&self) -> Option<&ExternalField> {
        self.frames.get(self.index).map(|f| &f.1)
    }
    pub(crate) fn validate_scale(&self, s: &Session, scale: f64) -> Result<()> {
        for (_, field) in self.frames.iter() {
            crate::external::validate_scale(s, scale, field)?;
        }
        Ok(())
    }

    fn seek(&mut self, index: usize) -> Result<()> {
        let frame = self.frames.get(index).ok_or_else(|| error("frame index outside this series"))?;
        self.index = index;
        self.position = frame.0;
        Ok(())
    }

    fn advance(&mut self, elapsed: f64) {
        if !self.playing || !elapsed.is_finite() || elapsed <= 0. {
            return;
        }
        let (Some(first), Some(last)) = (self.frames.first(), self.frames.last()) else { return };
        let span = last.0 - first.0;
        self.position = (self.position + span * (elapsed / PLAY_SECONDS).min(1.)).min(last.0);
        self.index = self.frames.partition_point(|f| f.0 <= self.position).saturating_sub(1);
        if self.position >= last.0 {
            self.playing = false;
        }
    }
}
impl Session {
    pub fn external_series(&self) -> Option<&Series> {
        self.field_series.as_ref().filter(|series| series.revision == self.revision() && self.results_view.is_none())
    }

    pub fn advance_external_series(&mut self, elapsed: f64) {
        if self.external_series().is_some_and(Series::playing)
            && let Some(series) = &mut self.field_series
        {
            series.advance(elapsed);
        }
    }
}

fn error(message: &str) -> EngineError {
    EngineError::Params(message.into())
}

fn control(s: &mut Session, p: &Value) -> Result<Value> {
    if s.external_series().is_none() {
        return Err(error("open an external series for the current mesh first"));
    }
    let series = s.field_series.as_mut().ok_or_else(|| error("missing external series"))?;
    let frame = opt::<usize>(p, "frame")?;
    let playing = opt::<bool>(p, "playing")?;
    if let Some(frame) = frame {
        series.seek(frame)?;
        series.playing = false;
    }
    if let Some(playing) = playing {
        if playing && frame.is_none() && series.frame() + 1 == series.count() {
            series.seek(0)?;
        }
        series.playing = playing;
    }
    Ok(series.with_position(json!({"frame":series.frame(),"frames":series.count(),"playing":series.playing()})))
}

fn load(s: &mut Session, p: &Value) -> Result<Value> {
    let selected = Selection::resolve(s, p, Kind::Series)?;
    let folder = Path::new(&selected.path).parent().ok_or_else(|| error("missing series directory"))?;
    let bytes = file::read_file(&selected.path, Kind::Series.limit())?;
    let source: SeriesFile = serde_json::from_slice(&bytes).map_err(|e| EngineError::Params(format!("invalid series: {e}")))?;
    let hash = fieldcraft_io::sha256_hex(&fieldcraft_io::mesh::encode(&s.model)?);
    let count = source.frames.len();
    if source.mesh_sha256 != hash {
        return Err(error("series belongs to a different mesh (mesh.bin SHA-256 mismatch)"));
    }
    let axis = Axis::read(&source.format, source.time_unit.as_deref(), source.axis)?;
    let cost = if source.components == 3 && source.unit == "m" { 3 } else { 1 };
    if !(2..=MAX_FRAMES).contains(&count) || s.model.nodes().len().saturating_mul(count) > MAX_SAMPLES / cost {
        return Err(error("external series exceeds the frame or sample bounds, or has fewer than two frames"));
    }
    let (first, _) = axis.sample(source.frames.first().ok_or_else(|| error("empty series"))?)?;
    let mut names = BTreeSet::new();
    let mut previous = None;
    let mut remaining = MAX_TOTAL_BYTES;
    let mut frames = Vec::with_capacity(count);
    let mut range = [f64::INFINITY, f64::NEG_INFINITY];
    let component = opt::<&str>(p, "component")?.filter(|v| *v != "auto");
    let mut scale = 0.;
    for entry in &source.frames {
        let (value, file_name) = axis.sample(entry)?;
        if !value.is_finite() || !(value - first).is_finite() || previous.is_some_and(|p| value <= p) || !names.insert(file_name) {
            return Err(error("frames need distinct files and strictly increasing finite axis values"));
        }
        previous = Some(value);
        let frame_path = sibling(s, folder, file_name, Kind::Field.limit())?;
        let bytes = file::read_file(&frame_path, remaining.min(64 * 1024 * 1024))?;
        remaining = remaining.saturating_sub(bytes.len() as u64);
        let data: FieldFile = serde_json::from_slice(&bytes).map_err(|e| EngineError::Params(format!("invalid frame {file_name}: {e}")))?;
        if !data.matches(&source.name, &source.unit, source.components) {
            return Err(error("frame field, unit or component count changed"));
        }
        let contour = data.prepare(s.model.nodes().keys(), &hash, component).map_err(error)?;
        scale = deformation_scale(s, p, &contour)?;
        range[0] = range[0].min(contour.min.1);
        range[1] = range[1].max(contour.max.1);
        frames.push((value, ExternalField::new(contour, s.revision())));
    }
    if !(range[1] - range[0]).is_finite() {
        return Err(error("series colour range cannot be represented"));
    }
    for (_, field) in &mut frames {
        field.set_range(range);
    }
    selected.register(s, &source.name, &source.unit)?;
    let series = Series { frames: Arc::new(frames), axis, revision: s.revision(), index: 0, position: first, playing: false };
    let reply = series.with_position(json!({"shown":true,"field":source.name,"frames":count,"range":range}));
    s.field_series = Some(series);
    s.external_deformation_scale = scale;
    s.external_field = None;
    s.results_view = None;
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn voltage_frames_keep_the_parameter_unit_and_validate_displacement_before_replacing_the_series() {
        let (_dir, seed) = crate::tests::design_file(b"{}");
        let path = Path::new(&seed).with_file_name("sweep.json");
        let mut s = Session::default();
        s.model.set_node(7, [0., 0., 0.]).unwrap();
        let mesh = fieldcraft_io::sha256_hex(&fieldcraft_io::mesh::encode(&s.model).unwrap());
        let mut frames = Vec::new();
        for (i, (value, z)) in [(-4., 0.), (1., 1.), (16., 1.2e38)].into_iter().enumerate() {
            let file = format!("u-{i}.json");
            let data = json!({"format":"fieldcraft-nodal-field/1","mesh_sha256":mesh,"name":"U","unit":"m","components":3,
                "node_ids":[7],"values":[0.,0.,z]});
            fs::write(path.with_file_name(&file), data.to_string()).unwrap();
            frames.push(json!({"file":file,"parameter":value}));
        }
        let axis = json!({"label":"Voltage","unit":"V"});
        let manifest =
            json!({"format":"fieldcraft-nodal-series/2","name":"U","unit":"m","components":3,"mesh_sha256":mesh,"axis":axis,"frames":frames});
        fs::write(&path, manifest.to_string()).unwrap();
        let params = json!({"path":path, "deformation_scale":2.});
        let first = load(&mut s, &params).unwrap();
        assert_eq!(first["axis"], axis);
        assert_eq!(first["parameter"], -4.);
        assert!(first.get("time").is_none());
        assert_eq!(s.external_deformation_scale, 2.);
        control(&mut s, &json!({"playing":true})).unwrap();
        s.advance_external_series(1.1);
        let next = control(&mut s, &json!({})).unwrap();
        assert_eq!(next, json!({"frame":1,"frames":3,"parameter":1.,"axis":axis,"playing":true}));
        let series = s.external_series().unwrap();
        assert!(!series.is_time());
        assert_eq!((series.axis_label(), series.axis_unit(), series.frame_value()), ("Voltage", "V", 1.));
        // The displayed frame accepts this scale, but the final displacement exceeds f32.
        assert!(crate::external::validate_scale(&s, 3., series.field().unwrap()).is_ok());
        assert!(series.validate_scale(&s, 3.).is_err());
        for (pointer, bad) in [
            ("/format", json!("fieldcraft-nodal-series/1")),
            ("/axis/label", json!(" ")),
            ("/axis/unit", json!("V\n")),
            ("/frames/1", json!({"time":1.,"file":"u-1.json"})),
            ("/frames/1", json!({"time":1.,"parameter":1.,"file":"u-1.json"})),
            ("/frames/1/parameter", json!(-5.)),
        ] {
            let mut damaged = manifest.clone();
            *damaged.pointer_mut(pointer).unwrap() = bad;
            fs::write(&path, damaged.to_string()).unwrap();
            assert!(load(&mut s, &params).is_err(), "{pointer}");
            assert_eq!(s.external_series().unwrap().frame_value(), 1.);
            assert_eq!(s.external_deformation_scale, 2.);
        }
        fs::write(&path, manifest.to_string()).unwrap();
        assert!(load(&mut s, &json!({"path":path,"deformation_scale":-1.})).is_err());
        assert_eq!(s.external_series().unwrap().frame_value(), 1.);
        assert_eq!(s.external_deformation_scale, 2.);
        load(&mut s, &json!({"path":path})).unwrap();
        assert_eq!(s.external_deformation_scale, 0.);
    }

    #[test]
    fn signed_nonuniform_frames_are_verified_and_played_on_one_scale() {
        let mut s = Session::default();
        assert!(matches!(s.execute("results.external.playback", &json!({})), Err(EngineError::Unavailable(_))));
        s.model.set_node(7, [0., 0., 0.]).unwrap();
        let dir = std::env::temp_dir().join(format!("fieldcraft-series-{}-{:?}", std::process::id(), std::thread::current().id()));
        fs::create_dir_all(&dir).unwrap();
        let mesh = fieldcraft_io::sha256_hex(&fieldcraft_io::mesh::encode(&s.model).unwrap());
        let mut frames = Vec::new();
        for (i, (t, y)) in [(0., -3.), (0.25, 2.), (1., 4.)].into_iter().enumerate() {
            let file = format!("frame-{i}.json");
            let data = json!({"format":"fieldcraft-nodal-field/1","mesh_sha256":mesh,"name":"B","unit":"T","components":3,"node_ids":[7],"values":[0.,y,0.]});
            fs::write(dir.join(&file), data.to_string()).unwrap();
            frames.push(json!({"time":t,"file":file}));
        }
        let path = dir.join("series.json");
        let mut manifest =
            json!({"format":"fieldcraft-nodal-series/1","name":"B","unit":"T","components":3,"mesh_sha256":mesh,"time_unit":"s","frames":frames});
        let params = json!({"path":path,"component":"y"});
        fs::write(&path, manifest.to_string()).unwrap();
        let first = load(&mut s, &params).unwrap();
        assert_eq!(first["range"], json!([-3., 4.]));
        assert_eq!(first["time_unit"], "s");
        assert_eq!(first["time"], 0.);
        assert!(first.get("axis").is_none());
        let series = s.external_series().unwrap();
        assert!(series.is_time());
        assert_eq!((series.axis_label(), series.axis_unit()), ("Time", "s"));
        assert!(load(&mut s, &json!({"path":path,"deformation_scale":1.})).is_err());
        assert_eq!(s.external_contour().unwrap().min.1, -3.);
        s.execute("results.external.playback", &json!({"playing":true})).unwrap();
        s.advance_external_series(1.2);
        assert_eq!(s.external_series().unwrap().frame_time(), 0.25);
        assert_eq!(s.external_contour().unwrap().max.1, 2.);
        assert_eq!(s.external_contour().unwrap().range, [-3., 4.]);
        s.advance_external_series(4.);
        assert!(!s.external_series().unwrap().playing());
        assert_eq!(s.external_series().unwrap().frame(), 2);
        control(&mut s, &json!({"frame":1})).unwrap();
        assert!(control(&mut s, &json!({"frame":3})).is_err());
        for (key, bad) in [("file", json!("../outside")), ("time", json!(0.)), ("file", json!("frame-0.json"))] {
            let mut damaged = manifest.clone();
            damaged["frames"][1][key] = bad;
            fs::write(&path, damaged.to_string()).unwrap();
            assert!(load(&mut s, &params).is_err());
            assert_eq!(s.external_series().unwrap().frame(), 1);
        }
        manifest["unit"] = json!("mT");
        fs::write(&path, manifest.to_string()).unwrap();
        assert!(load(&mut s, &params).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
