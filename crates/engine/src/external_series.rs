//! Playback of measured or computed nodal fields on a fixed mesh.
//! All frames are validated before the session changes. Playback only reads memory.
use crate::cmd::{file, forms, opt};
use crate::external::ExternalField;
use crate::external_catalog::{Kind, Selection, sibling};
use crate::requirement::Requirement;
use crate::{CommandKind, CommandSpec, EngineError, Result, Session};
use fieldcraft_render::nodal::FieldFile;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

pub(crate) const SERIES: CommandSpec = CommandSpec {
    id: "results.external.series",
    label: "Open External Field Series",
    params: r#"{"path":"B.series.json","component"?: "auto|magnitude|x|y|z"} — open a mesh-bound time series on its first frame. A selected vector component has one colour scale across all frames. Use {"entry":"name"} instead of path to reopen a checked catalog row. Use results.external.playback to seek or play."#,
    form: Some(forms::EXTERNAL_SERIES),
    kind: CommandKind::Session,
    requires: &[Requirement::Mesh],
    run: load,
};
pub(crate) const PLAYBACK: CommandSpec = CommandSpec {
    id: "results.external.playback",
    label: "External Field Playback",
    params: r#"{"frame"?: 0,"playing"?: true} — select a zero-based frame and pause, or play through the recorded timestamps in four display seconds. Starting at the last frame replays from the first. With no parameters, report the current frame and time in seconds."#,
    form: Some(forms::EXTERNAL_PLAYBACK),
    kind: CommandKind::Session,
    requires: &[Requirement::ExternalSeries],
    run: control,
};

pub(crate) const MAX_FRAMES: usize = 4096;
// Bound retained per-frame labels and scalars (at most 96 MiB plus frame metadata).
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
    time_unit: String,
    frames: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    time: f64,
    file: String,
}

#[derive(Clone, Debug)]
pub struct Series {
    frames: Arc<Vec<(f64, ExternalField)>>,
    revision: u64,
    index: usize,
    time: f64,
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
    pub fn frame_time(&self) -> f64 {
        self.frames.get(self.index).map_or(self.time, |f| f.0)
    }
    pub(crate) fn field(&self) -> Option<&ExternalField> {
        self.frames.get(self.index).map(|f| &f.1)
    }

    fn seek(&mut self, index: usize) -> Result<()> {
        let frame = self.frames.get(index).ok_or_else(|| error("frame index outside this series"))?;
        self.index = index;
        self.time = frame.0;
        Ok(())
    }

    fn advance(&mut self, elapsed: f64) {
        if !self.playing || !elapsed.is_finite() || elapsed <= 0. {
            return;
        }
        let (Some(first), Some(last)) = (self.frames.first(), self.frames.last()) else { return };
        let span = last.0 - first.0;
        self.time = (self.time + span * (elapsed / PLAY_SECONDS).min(1.)).min(last.0);
        self.index = self.frames.partition_point(|f| f.0 <= self.time).saturating_sub(1);
        if self.time >= last.0 {
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
    Ok(json!({"frame":series.frame(),"frames":series.count(),"time":series.frame_time(),"time_unit":"s","playing":series.playing()}))
}

fn load(s: &mut Session, p: &Value) -> Result<Value> {
    let selected = Selection::resolve(s, p, Kind::Series)?;
    let folder = Path::new(&selected.path).parent().ok_or_else(|| error("missing series directory"))?;
    let bytes = file::read_file(&selected.path, Kind::Series.limit())?;
    let source: SeriesFile = serde_json::from_slice(&bytes).map_err(|e| EngineError::Params(format!("invalid series: {e}")))?;
    let hash = fieldcraft_io::sha256_hex(&fieldcraft_io::mesh::encode(&s.model)?);
    let count = source.frames.len();
    if source.format != "fieldcraft-nodal-series/1" || source.mesh_sha256 != hash || source.time_unit != "s" {
        return Err(error("expected a fieldcraft-nodal-series/1 in seconds on this mesh"));
    }
    if !(2..=MAX_FRAMES).contains(&count) || s.model.nodes().len().saturating_mul(count) > MAX_SAMPLES {
        return Err(error("external time series exceeds the frame or sample bounds, or has fewer than two frames"));
    }
    let Some(first) = source.frames.first().map(|f| f.time) else { return Err(error("empty series")) };
    let mut names = BTreeSet::new();
    let mut previous = None;
    let mut remaining = MAX_TOTAL_BYTES;
    let mut frames = Vec::with_capacity(count);
    let mut range = [f64::INFINITY, f64::NEG_INFINITY];
    let component = opt::<&str>(p, "component")?.filter(|v| *v != "auto");
    for entry in &source.frames {
        if !entry.time.is_finite() || !(entry.time - first).is_finite() || previous.is_some_and(|t| entry.time <= t) || !names.insert(&entry.file) {
            return Err(error("frames need distinct files and strictly increasing finite times"));
        }
        previous = Some(entry.time);
        let frame_path = sibling(s, folder, &entry.file, Kind::Field.limit())?;
        let bytes = file::read_file(&frame_path, remaining.min(64 * 1024 * 1024))?;
        remaining = remaining.saturating_sub(bytes.len() as u64);
        let data: FieldFile = serde_json::from_slice(&bytes).map_err(|e| EngineError::Params(format!("invalid frame {}: {e}", entry.file)))?;
        if !data.matches(&source.name, &source.unit, source.components) {
            return Err(error("frame field, unit or component count changed"));
        }
        let contour = data.prepare(s.model.nodes().keys(), &hash, component).map_err(error)?;
        range[0] = range[0].min(contour.min.1);
        range[1] = range[1].max(contour.max.1);
        frames.push((entry.time, ExternalField::new(contour, s.revision())));
    }
    if !(range[1] - range[0]).is_finite() {
        return Err(error("series colour range cannot be represented"));
    }
    for (_, field) in &mut frames {
        field.set_range(range);
    }
    selected.register(s, &source.name, &source.unit)?;
    s.field_series = Some(Series { frames: Arc::new(frames), revision: s.revision(), index: 0, time: first, playing: false });
    s.external_field = None;
    s.results_view = None;
    Ok(json!({"shown":true,"field":source.name,"frames":count,"time":first,"time_unit":"s","range":range}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
        assert_eq!(load(&mut s, &params).unwrap()["range"], json!([-3., 4.]));
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
