//! Little-endian storage for imported visualization frames. Every count determines exactly
//! how many bytes follow. The reader charges the full in-memory cost of a collection before
//! it allocates and checks its span against the bytes actually present.

use super::{
    CELL_BYTES, Cell, FIELD_BYTES, FRAME_BYTES, Field, Frame, Grid, MAGIC, MAX_BYTES, MAX_CELLS, MAX_COMPONENTS, MAX_FIELDS, MAX_FRAME_BYTES,
    MAX_FRAMES, MAX_POINTS, PART, Series, cell_arity, storage_limit, valid_name,
};
use crate::{Error, Result, invalid};
use fieldcraft_model::units::Unit;
use std::collections::BTreeMap;

const MAGIC_WITH_UNITS: &[u8; 8] = b"FCEXT002";

pub(crate) fn encode(series: &Series) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    append_series(&mut bytes, series)?;
    Ok(bytes)
}

fn append_series(bytes: &mut Vec<u8>, series: &Series) -> Result<()> {
    series.check()?;
    let with_units = series.frames.iter().any(|f| f.displacement_unit.is_some());
    bytes.extend_from_slice(if with_units { MAGIC_WITH_UNITS } else { MAGIC });
    bytes.extend_from_slice(&(series.frames.len() as u32).to_le_bytes());
    for frame in &series.frames {
        if with_units {
            let symbol = frame.displacement_unit.as_ref().map_or("", Unit::symbol);
            let len = u16::try_from(symbol.len()).map_err(|_| Error::Invalid("external length unit is too long".into()))?;
            bytes.extend(len.to_le_bytes());
            bytes.extend_from_slice(symbol.as_bytes());
        }
        bytes.extend_from_slice(&frame.time.to_le_bytes());
        bytes.extend_from_slice(&(frame.grid.points.len() as u32).to_le_bytes());
        for value in frame.grid.points.iter().flatten() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&(frame.grid.cells.len() as u32).to_le_bytes());
        for cell in &frame.grid.cells {
            bytes.push(cell.vtk_type);
            for point in &cell.points {
                bytes.extend_from_slice(&point.to_le_bytes());
            }
        }
        for data in [&frame.point_data, &frame.cell_data] {
            bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
            for (name, field) in data {
                bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
                bytes.extend_from_slice(name.as_bytes());
                bytes.push(field.components);
                for value in &field.values {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
    Ok(())
}

struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let (part, rest) = self.rest.split_at_checked(n).ok_or_else(|| Error::Invalid(format!("{PART}: incomplete frame data")))?;
        self.rest = rest;
        Ok(part)
    }

    fn number<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.take(N)?.try_into().map_err(|_| Error::Invalid(format!("{PART}: incomplete number")))
    }

    fn words<const N: usize>(&mut self, count: usize) -> Result<&'a [[u8; N]]> {
        let bytes = self.take(count.checked_mul(N).ok_or_else(storage_limit)?)?;
        Ok(bytes.as_chunks::<N>().0)
    }

    fn count(&mut self, max: usize, what: &str) -> Result<usize> {
        let n = u32::from_le_bytes(self.number()?) as usize;
        if !(1..=max).contains(&n) {
            return invalid(format!("{PART}: expected 1 to {max} {what}"));
        }
        Ok(n)
    }

    fn fields(&mut self, tuples: usize, max_fields: usize, remaining: &mut usize) -> Result<BTreeMap<String, Field>> {
        let count = usize::from(u16::from_le_bytes(self.number()?));
        if count > max_fields {
            return invalid(format!("{PART}: too many fields"));
        }
        super::charge_storage(remaining, count * FIELD_BYTES)?;
        let mut fields = BTreeMap::new();
        for _ in 0..count {
            let length = usize::from(u16::from_le_bytes(self.number()?));
            let name = std::str::from_utf8(self.take(length)?).map_err(|e| Error::Invalid(format!("{PART}: invalid field name: {e}")))?;
            if !valid_name(name) || fields.contains_key(name) {
                return invalid(format!("{PART}: invalid or duplicate field name"));
            }
            let [components] = self.number()?;
            if !(1..=MAX_COMPONENTS).contains(&usize::from(components)) {
                return invalid(format!("{PART}: invalid field component count"));
            }
            let count = tuples.checked_mul(usize::from(components)).ok_or_else(storage_limit)?;
            super::charge_storage(remaining, length.saturating_add(count.saturating_mul(8)))?;
            let values = self.words::<8>(count)?.iter().copied().map(f64::from_le_bytes).collect();
            fields.insert(name.to_string(), Field { components, values });
        }
        Ok(fields)
    }

    fn frame(&mut self, remaining: &mut usize, with_units: bool) -> Result<Frame> {
        let budget = (*remaining).min(MAX_FRAME_BYTES);
        let mut left = budget;
        super::charge_storage(&mut left, FRAME_BYTES)?;
        let displacement_unit = if with_units {
            let len = usize::from(u16::from_le_bytes(self.number()?));
            super::charge_storage(&mut left, len)?;
            let symbol = std::str::from_utf8(self.take(len)?).map_err(|e| Error::Invalid(e.to_string()))?;
            if symbol.is_empty() { None } else { Some(Unit::parse(symbol).map_err(|e| Error::Invalid(e.to_string()))?) }
        } else {
            None
        };
        let time = f64::from_le_bytes(self.number()?);
        let np = self.count(MAX_POINTS, "points")?;
        super::charge_storage(&mut left, np.saturating_mul(24))?;
        let xyz = self.words::<8>(np.checked_mul(3).ok_or_else(storage_limit)?)?;
        let points = xyz.as_chunks::<3>().0.iter().map(|coords| coords.map(f64::from_le_bytes)).collect();
        let nc = self.count(MAX_CELLS, "cells")?;
        if nc > self.rest.len() / 5 {
            return invalid(format!("{PART}: truncated cell list"));
        }
        super::charge_storage(&mut left, nc.saturating_mul(CELL_BYTES))?;
        let mut cells = Vec::with_capacity(nc);
        for _ in 0..nc {
            let [vtk_type] = self.number()?;
            let arity = cell_arity(vtk_type).ok_or_else(|| Error::Invalid(format!("{PART}: unsupported VTK cell type {vtk_type}")))?;
            super::charge_storage(&mut left, arity * 4)?;
            let points = self.words::<4>(arity)?.iter().copied().map(u32::from_le_bytes).collect();
            cells.push(Cell { vtk_type, points });
        }
        let point_data = self.fields(np, MAX_FIELDS, &mut left)?;
        let cell_data = self.fields(nc, MAX_FIELDS - point_data.len(), &mut left)?;
        let frame = Frame { time, displacement_unit, grid: Grid { points, cells }, point_data, cell_data };
        frame.check()?;
        *remaining -= budget - left;
        Ok(frame)
    }
}

#[cfg(test)]
pub(crate) fn decode(bytes: &[u8]) -> Result<Series> {
    decode_limited(bytes, MAX_BYTES, MAX_FRAMES)
}

fn decode_limited(bytes: &[u8], limit: usize, frame_limit: usize) -> Result<Series> {
    if bytes.len() > MAX_BYTES {
        return Err(storage_limit());
    }
    let with_units = bytes.starts_with(MAGIC_WITH_UNITS);
    let magic = if with_units { MAGIC_WITH_UNITS } else { MAGIC };
    let body = bytes.strip_prefix(magic).ok_or_else(|| Error::Invalid(format!("{PART}: unknown external results encoding")))?;
    let mut reader = Reader { rest: body };
    let count = reader.count(frame_limit, "frames")?;
    let mut remaining = limit;
    let mut frames = Vec::new();
    for _ in 0..count {
        frames.push(reader.frame(&mut remaining, with_units)?);
    }
    if !reader.rest.is_empty() {
        return invalid(format!("{PART}: unexpected bytes after last frame"));
    }
    let series = Series { frames };
    series.check_timeline()?;
    Ok(series)
}

const CATALOG_MAGIC: &[u8; 8] = b"FCEXT003";

pub(crate) fn encode_catalog(catalog: &super::Catalog) -> Result<Vec<u8>> {
    if catalog.iter().count() == 1
        && let Some(default) = catalog.get("")
    {
        return encode(&default.series);
    }
    let mut out = CATALOG_MAGIC.to_vec();
    out.extend_from_slice(&(catalog.iter().count() as u32).to_le_bytes());
    for (key, data) in catalog.iter() {
        for text in [key, data.design.as_deref().unwrap_or("")] {
            out.extend_from_slice(&(text.len() as u16).to_le_bytes());
            out.extend_from_slice(text.as_bytes());
        }
        let size_at = out.len();
        out.extend_from_slice(&0u32.to_le_bytes());
        let start = out.len();
        append_series(&mut out, &data.series)?;
        if out.len() > MAX_BYTES {
            return Err(storage_limit());
        }
        let size = u32::try_from(out.len() - start).map_err(|_| storage_limit())?;
        out.get_mut(size_at..start).ok_or_else(storage_limit)?.copy_from_slice(&size.to_le_bytes());
    }
    Ok(out)
}

pub(crate) fn decode_catalog(bytes: &[u8]) -> Result<super::Catalog> {
    if bytes.len() > MAX_BYTES {
        return Err(storage_limit());
    }
    let mut catalog = super::Catalog::default();
    let Some(body) = bytes.strip_prefix(CATALOG_MAGIC) else {
        catalog.insert(String::new(), None, decode_limited(bytes, MAX_BYTES, MAX_FRAMES)?)?;
        return Ok(catalog);
    };
    let mut reader = Reader { rest: body };
    let count = reader.count(MAX_FRAMES, "datasets")?;
    if count == 0 {
        return invalid("empty external result catalog");
    }
    for _ in 0..count {
        let mut text = || -> Result<&str> {
            let len = usize::from(u16::from_le_bytes(reader.number()?));
            if len > 256 {
                return invalid("external result dataset label is too long");
            }
            std::str::from_utf8(reader.take(len)?).map_err(|e| Error::Invalid(e.to_string()))
        };
        let key = text()?;
        let digest = text()?;
        if catalog.get(key).is_some() {
            return invalid("duplicate external result dataset");
        }
        let size = reader.count(MAX_BYTES, "dataset bytes")?;
        let metadata = super::Catalog::metadata_bytes(key, Some(digest));
        let remaining = catalog.remaining_for(key).checked_sub(metadata).ok_or_else(storage_limit)?;
        let series = decode_limited(reader.take(size)?, remaining, MAX_FRAMES - catalog.frame_count())?;
        catalog.insert(key.into(), (!digest.is_empty()).then(|| digest.into()), series)?;
    }
    if !reader.rest.is_empty() {
        return invalid("unexpected bytes after last external result dataset");
    }
    Ok(catalog)
}
