//! External result grids and time collections, from Kitware's
//! [VTK XML File Formats](https://docs.vtk.org/en/latest/vtk_file_formats/vtkxml_file_format.html).
//! A piece's point index is its identity. Coincident points can carry different field values.
//! In a PVD collection every frame owns its grid, including the coordinates at that time.

mod array;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::BTreeMap;

use roxmltree::{Document, Node, ParsingOptions};

use crate::external::{Cell, Field, Frame, Grid, MAX_CELLS, MAX_COMPONENTS, MAX_FIELDS, MAX_FRAME_BYTES, MAX_FRAMES, MAX_POINTS, cell_arity};
use crate::{Error, Result};

fn bad(message: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("VTK XML: {message}"))
}

fn utf8(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| bad("invalid UTF-8 in XML metadata"))
}

fn document(xml: &str) -> Result<Document<'_>> {
    // Both the metadata tree and the result arrays have independent allocation limits.
    let options = ParsingOptions { nodes_limit: 64_000, ..Default::default() };
    Document::parse_with_options(xml, options).map_err(bad)
}

fn root<'a>(doc: &'a Document<'_>, kind: &str) -> Result<Node<'a, 'a>> {
    let root = doc.root_element();
    if !root.has_tag_name("VTKFile") || root.attribute("type") != Some(kind) {
        return Err(bad(format!("expected VTKFile of type {kind}")));
    }
    Ok(root)
}

fn one<'a, 'i>(parent: Node<'a, 'i>, tag: &str) -> Result<Node<'a, 'i>> {
    let mut items = parent.children().filter(|child| child.has_tag_name(tag));
    let node = items.next().ok_or_else(|| bad(format!("missing {tag}")))?;
    if items.next().is_some() {
        return Err(bad(format!("multiple {tag} elements are not supported")));
    }
    Ok(node)
}

fn attr<'a>(node: Node<'a, '_>, name: &str) -> Result<&'a str> {
    node.attribute(name).ok_or_else(|| bad(format!("missing {name} on {}", node.tag_name().name())))
}

fn count(node: Node<'_, '_>, name: &str, limit: usize) -> Result<usize> {
    let n = attr(node, name)?.parse::<usize>().map_err(bad)?;
    if n > limit {
        return Err(Error::Limit(format!("{name} exceeds {limit}")));
    }
    Ok(n)
}

fn metadata(bytes: &[u8]) -> Result<(Cow<'_, str>, Option<&[u8]>)> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(Error::Limit("VTU file too large".into()));
    }
    // Raw VTK payloads are not XML character data. Keep their bytes out of the XML parser.
    let open = b"<AppendedData";
    let close = b"</AppendedData>";
    let Some(tag) = bytes.windows(open.len()).position(|w| w == open) else {
        return Ok((Cow::Borrowed(utf8(bytes)?), None));
    };
    let after = bytes.get(tag..).ok_or_else(|| bad("invalid AppendedData"))?;
    let begin = after.iter().position(|&b| b == b'>').ok_or_else(|| bad("incomplete AppendedData tag"))? + tag + 1;
    let end = bytes.windows(close.len()).rposition(|w| w == close).ok_or_else(|| bad("missing AppendedData closing tag"))?;
    let body = bytes.get(begin..end).ok_or_else(|| bad("invalid AppendedData bounds"))?;
    let sentinel = body.iter().position(|b| !b.is_ascii_whitespace()).ok_or_else(|| bad("missing AppendedData sentinel"))?;
    if body.get(sentinel) != Some(&b'_') {
        return Err(bad("AppendedData must start with '_'"));
    }
    let mut xml = utf8(bytes.get(..begin).ok_or_else(|| bad("invalid XML prefix"))?)?.to_owned();
    xml.push_str(utf8(bytes.get(end..).ok_or_else(|| bad("invalid XML suffix"))?)?);
    Ok((Cow::Owned(xml), body.get(sentinel + 1..)))
}

fn components(node: Node<'_, '_>) -> Result<usize> {
    let n = node.attribute("NumberOfComponents").unwrap_or("1").parse::<usize>().map_err(bad)?;
    if !(1..=MAX_COMPONENTS).contains(&n) {
        return Err(bad(format!("unsupported component count {n}")));
    }
    Ok(n)
}

fn cell_array<'a, 'i>(cells: Node<'a, 'i>, name: &str) -> Result<Node<'a, 'i>> {
    let mut arrays = cells.children().filter(|n| n.has_tag_name("DataArray") && n.attribute("Name") == Some(name));
    let node = arrays.next().ok_or_else(|| bad(format!("missing Cells array {name}")))?;
    if arrays.next().is_some() || components(node)? != 1 {
        return Err(bad(format!("expected one scalar Cells array named {name}")));
    }
    Ok(node)
}

fn geometry(decoder: &mut array::Decoder<'_>, piece: Node<'_, '_>, np: usize, nc: usize, budget: &mut usize) -> Result<Grid> {
    crate::external::charge_storage(budget, crate::external::FRAME_BYTES)?;
    crate::external::charge_storage(budget, np.saturating_mul(24))?;
    crate::external::charge_storage(budget, nc.saturating_mul(crate::external::CELL_BYTES))?;
    let pos = one(one(piece, "Points")?, "DataArray")?;
    if components(pos)? != 3 {
        return Err(bad("Points must have 3 components"));
    }
    let coords = decoder.values(pos, 3 * np, false)?;
    let points = coords.as_chunks::<3>().0.to_vec();
    drop(coords);
    let source = one(piece, "Cells")?;
    let types = decoder.indices(cell_array(source, "types")?, nc)?;
    let mut kinds = Vec::with_capacity(nc);
    let mut entries = 0_usize;
    for number in types {
        let kind = u8::try_from(number).map_err(|_| bad(format!("unsupported VTK cell type {number}")))?;
        let arity = cell_arity(kind).ok_or_else(|| bad(format!("unsupported VTK cell type {kind}")))?;
        entries = entries.checked_add(arity).ok_or_else(|| bad("cell connectivity too large"))?;
        kinds.push((kind, entries));
    }
    crate::external::charge_storage(budget, entries.saturating_mul(4))?;
    let offsets = decoder.indices(cell_array(source, "offsets")?, nc)?;
    if !offsets.iter().zip(&kinds).all(|(&offset, &(_, end))| offset as usize == end) {
        return Err(bad("cell offsets do not match the cell types"));
    }
    drop(offsets);
    let indices = decoder.indices(cell_array(source, "connectivity")?, entries)?;
    if indices.iter().any(|&index| index as usize >= np) {
        return Err(bad("cell references a missing point"));
    }
    let mut start = 0;
    let mut cells = Vec::with_capacity(nc);
    for (vtk_type, end) in kinds {
        let slice = indices.get(start..end).ok_or_else(|| bad("invalid cell connectivity"))?;
        cells.push(Cell { vtk_type, points: slice.to_vec() });
        start = end;
    }
    Ok(Grid { points, cells })
}

fn fields(
    decoder: &mut array::Decoder<'_>,
    piece: Node<'_, '_>,
    tag: &str,
    tuples: usize,
    remaining: usize,
    budget: &mut usize,
) -> Result<BTreeMap<String, Field>> {
    let mut result = BTreeMap::new();
    if !piece.children().any(|n| n.has_tag_name(tag)) {
        return Ok(result);
    }
    for node in one(piece, tag)?.children().filter(Node::is_element) {
        if !node.has_tag_name("DataArray") {
            return Err(bad(format!("unsupported {} in {tag}", node.tag_name().name())));
        }
        let name = attr(node, "Name")?;
        if !crate::external::valid_name(name) || result.contains_key(name) {
            return Err(bad(format!("empty, oversized, or duplicate array name in {tag}")));
        }
        if result.len() >= remaining {
            return Err(Error::Limit("too many VTU fields".into()));
        }
        let n = components(node)?;
        let entries = tuples.checked_mul(n).ok_or_else(|| bad("field is too large"))?;
        crate::external::charge_storage(budget, crate::external::FIELD_BYTES.saturating_add(name.len()).saturating_add(entries.saturating_mul(8)))?;
        let values = decoder.values(node, entries, false)?;
        result.insert(name.to_owned(), Field { components: n as u8, values });
    }
    Ok(result)
}

/// Read one serial unstructured grid. Time is supplied by the collection or its caller.
pub fn read(bytes: &[u8], time: f64) -> Result<Frame> {
    read_with_budget(bytes, time, MAX_FRAME_BYTES)
}

/// Read one frame within the storage still available to its series.
pub fn read_with_budget(bytes: &[u8], time: f64, remaining: usize) -> Result<Frame> {
    let mut budget = remaining.min(MAX_FRAME_BYTES);
    if !time.is_finite() {
        return Err(bad("frame time must be finite"));
    }
    let (xml, payload) = metadata(bytes)?;
    let doc = document(&xml)?;
    let vtk = root(&doc, "UnstructuredGrid")?;
    let mut decoder = array::Decoder::new(vtk, payload)?;
    let piece = one(one(vtk, "UnstructuredGrid")?, "Piece")?;
    let np = count(piece, "NumberOfPoints", MAX_POINTS)?;
    let nc = count(piece, "NumberOfCells", MAX_CELLS)?;
    let grid = geometry(&mut decoder, piece, np, nc, &mut budget)?;
    let point_data = fields(&mut decoder, piece, "PointData", np, MAX_FIELDS, &mut budget)?;
    let cell_data = fields(&mut decoder, piece, "CellData", nc, MAX_FIELDS - point_data.len(), &mut budget)?;
    let frame = Frame { time, grid, point_data, cell_data };
    frame.check()?;
    Ok(frame)
}

/// Indexes only hold metadata and paths; numeric arrays belong to the VTU piece.
pub const MAX_INDEX_BYTES: usize = 4 << 20;

fn index_document(bytes: &[u8]) -> Result<Document<'_>> {
    if bytes.len() > MAX_INDEX_BYTES {
        return Err(Error::Limit("VTK index file exceeds 4 MiB".into()));
    }
    document(utf8(bytes)?)
}

fn reference(node: Node<'_, '_>, attribute: &str, extensions: &[&str]) -> Result<String> {
    let path = attr(node, attribute)?.replace('\\', "/");
    let permitted = std::path::Path::new(&path).extension().is_some_and(|e| extensions.iter().any(|ext| e.eq_ignore_ascii_case(ext)));
    if !permitted
        || path.len() > crate::MAX_PATH_LEN
        || path.starts_with('/')
        || path.contains(':')
        || path.chars().any(char::is_control)
        || path.split('/').any(|part| part == "..")
    {
        return Err(bad(format!("{attribute} must name a relative {} file within the index directory", extensions.join(" or "))));
    }
    Ok(path)
}

/// Resolve the one serial VTU piece in a PUnstructuredGrid index. The caller confines the
/// returned path to the index directory and reads the actual topology and arrays from the VTU.
pub fn single_piece(bytes: &[u8]) -> Result<String> {
    let doc = index_document(bytes)?;
    let grid = one(root(&doc, "PUnstructuredGrid")?, "PUnstructuredGrid")?;
    let mut pieces = grid.children().filter(|n| n.has_tag_name("Piece"));
    let piece = pieces.next().ok_or_else(|| bad("PVTU has no Piece"))?;
    if pieces.next().is_some() {
        return Err(bad("multi-piece PVTU is not supported; export one serial grid per time step"));
    }
    reference(piece, "Source", &["vtu"])
}

/// Read a serial PVD time collection. Members are grids or single-piece indexes.
pub fn collection(bytes: &[u8]) -> Result<Vec<(f64, String)>> {
    let doc = index_document(bytes)?;
    let vtk = root(&doc, "Collection")?;
    let mut frames = Vec::new();
    let mut previous_group = None;
    for entry in one(vtk, "Collection")?.children().filter(Node::is_element) {
        if !entry.has_tag_name("DataSet") || entry.attribute("part").unwrap_or("0") != "0" {
            return Err(bad("expected one VTU DataSet per time step; multipart PVD is not supported"));
        }
        let group = entry.attribute("group").unwrap_or("");
        if previous_group.replace(group).is_some_and(|previous| previous != group) {
            return Err(bad("multiple PVD groups are not supported"));
        }
        let time = attr(entry, "timestep")?.parse::<f64>().map_err(bad)?;
        if !time.is_finite() || frames.last().is_some_and(|(prev, _)| time <= *prev) {
            return Err(bad("PVD time steps must be finite and strictly increasing"));
        }
        let path = reference(entry, "file", &["vtu", "pvtu"])?;
        if frames.len() >= MAX_FRAMES {
            return Err(Error::Limit("too many PVD frames".into()));
        }
        frames.push((time, path));
    }
    if frames.is_empty() {
        return Err(bad("empty PVD collection"));
    }
    Ok(frames)
}
