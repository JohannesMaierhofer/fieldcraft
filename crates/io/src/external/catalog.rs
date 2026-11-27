//! Independent visualization series keyed by their explicitly selected CAD body.
//! The empty key is the standalone series, including all legacy projects.
use super::{MAX_BYTES, MAX_FRAMES, Series, storage_limit, valid_name};
use crate::{Result, invalid};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, PartialEq)]
pub struct Dataset {
    /// SHA-256 of the owning design, or none for the standalone series.
    pub design: Option<String>,
    pub series: Arc<Series>,
    storage: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    datasets: BTreeMap<String, Dataset>,
    storage: usize,
    frames: usize,
}

impl Catalog {
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Dataset)> {
        self.datasets.iter().map(|(key, dataset)| (key.as_str(), dataset))
    }

    pub fn get(&self, body: &str) -> Option<&Dataset> {
        self.datasets.get(body)
    }

    pub fn first(&self) -> Option<(&str, &Dataset)> {
        self.iter().next()
    }

    /// Payload retained only by this snapshot, excluding series shared with the current document.
    pub fn retained_bytes(&self, current: Option<&Self>) -> usize {
        self.iter()
            .filter(|(key, dataset)| !current.and_then(|c| c.get(key)).is_some_and(|other| Arc::ptr_eq(&dataset.series, &other.series)))
            .map(|(_, dataset)| dataset.storage)
            .sum()
    }

    pub fn frame_count(&self) -> usize {
        self.frames
    }

    pub fn storage_bytes(&self) -> usize {
        self.storage
    }

    pub fn remaining_for(&self, key: &str) -> usize {
        MAX_BYTES.saturating_sub(self.storage_bytes().saturating_sub(self.get(key).map_or(0, |d| d.storage)))
    }

    pub fn insert(&mut self, key: String, design: Option<String>, series: Series) -> Result<()> {
        let standalone = key.is_empty() && design.is_none();
        if !standalone && !(valid_name(&key) && design.as_ref().is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))) {
            return invalid("external body results need a valid body name and design digest");
        }
        series.check()?;
        let old_count = self.get(&key).map_or(0, |d| d.series.frames.len());
        if self.frame_count() - old_count + series.frames.len() > MAX_FRAMES {
            return invalid("too many external result frames across the model");
        }
        let metadata = Self::metadata_bytes(&key, design.as_deref());
        let storage = series.frames.iter().map(super::Frame::storage_bytes).fold(metadata, usize::saturating_add);
        let remaining = self.remaining_for(&key);
        if storage > remaining {
            return Err(storage_limit());
        }
        self.storage = MAX_BYTES - remaining + storage;
        self.frames = self.frames - old_count + series.frames.len();
        self.datasets.insert(key, Dataset { design, series: Arc::new(series), storage });
        Ok(())
    }

    pub fn metadata_bytes(key: &str, design: Option<&str>) -> usize {
        // Legacy standalone files can already occupy the full original frame budget.
        if key.is_empty() { 0 } else { 256 + key.len() + design.map_or(0, str::len) }
    }
}
