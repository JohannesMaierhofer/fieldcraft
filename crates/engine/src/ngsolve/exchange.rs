//! Export existing FieldCraft cells and their named regions in SI and original label order.

use std::collections::{BTreeMap, BTreeSet};

use fieldcraft_mesh::predicates::{Point2, Point3, Sign, orient2d, tet_volume_sign};
use fieldcraft_model::{ElementKind, Model};
use serde::Serialize;

use super::{Analysis, invalid};
use crate::Result;

const MAX_CELLS: usize = 1_000_000;
const MAX_NODES: usize = 1_000_000;
type SideId = (u32, u8);

#[derive(Serialize)]
pub(super) struct Exchange<'a> {
    dimension: u8,
    mesh_sha256: &'a str,
    points_m: Vec<[f64; 3]>,
    node_ids: Vec<u32>,
    element_ids: Vec<u32>,
    cells: Vec<Vec<u32>>,
    material_names: &'a [String],
    material_ids: Vec<u32>,
    boundaries: Vec<Side>,
    boundary_names: &'a [String],
    interfaces: Vec<Side>,
    interface_names: &'a [String],
}

#[derive(Serialize)]
struct Side {
    nodes: Vec<u32>,
    owner: u32,
    group: u32,
    neighbor: Option<u32>,
}

struct Face {
    nodes: Vec<u32>,
    cell: u32,
}

fn connectivity(face: &Face) -> ([u32; 3], bool) {
    let mut key = [u32::MAX; 3];
    for (out, node) in key.iter_mut().zip(&face.nodes) {
        *out = *node;
    }
    let reversed = match key {
        [a, b, u32::MAX] => a > b,
        [a, b, c] => (a > b) ^ (a > c) ^ (b > c),
    };
    key.sort_unstable();
    (key, reversed)
}

fn verify_cell(label: u32, kind: ElementKind, coordinates: &[[f64; 3]]) -> Result<()> {
    let sign = match (kind, coordinates) {
        (ElementKind::Tet4, [[ax, ay, az], [bx, by, bz], [cx, cy, cz], [dx, dy, dz]]) => {
            tet_volume_sign(&Point3::new(*ax, *ay, *az)?, &Point3::new(*bx, *by, *bz)?, &Point3::new(*cx, *cy, *cz)?, &Point3::new(*dx, *dy, *dz)?)
        }
        (ElementKind::Tri3, [[ax, ay, az], [bx, by, bz], [cx, cy, cz]]) if *az == 0.0 && *bz == 0.0 && *cz == 0.0 => {
            orient2d(&Point2::new(*ax, *ay)?, &Point2::new(*bx, *by)?, &Point2::new(*cx, *cy)?)
        }
        _ => return Err(invalid(format!("Element {label} must be a TET4 solid or a TRI3 in the XY plane matching the analysis dimension"))),
    };
    if sign != Sign::Positive {
        return Err(invalid(format!("Element {label} is degenerate or inverted")));
    }
    Ok(())
}

fn assign<K: Ord + Copy>(
    names: &[String],
    mut members: impl FnMut(&str) -> Result<Vec<K>>,
    mut valid: impl FnMut(K) -> bool,
) -> Result<BTreeMap<K, u32>> {
    let mut groups = BTreeMap::new();
    for (index, name) in names.iter().enumerate() {
        let labels = members(name)?;
        if labels.is_empty() {
            return Err(invalid(format!("Set {name:?} is empty")));
        }
        let group = u32::try_from(index + 1).map_err(|_| invalid("Too many mesh groups"))?;
        for label in labels {
            if !valid(label) {
                return Err(invalid(format!("Set {name:?} contains an entity outside the requested mesh region")));
            }
            if groups.insert(label, group).is_some() {
                return Err(invalid(format!("Set {name:?} overlaps an assigned mesh group")));
            }
        }
    }
    Ok(groups)
}

pub(super) fn export<'a>(model: &Model, analysis: &'a Analysis, mesh_sha256: &'a str) -> Result<Exchange<'a>> {
    if model.elements().is_empty() || model.elements().len() > MAX_CELLS || model.nodes().len() > MAX_NODES {
        return Err(invalid("An NGSolve analysis needs a mesh with up to 1,000,000 cells and nodes"));
    }
    let planar = analysis.dimension == 2;
    let kind = if planar { ElementKind::Tri3 } else { ElementKind::Tet4 };
    let sides = if planar { kind.edges() } else { kind.faces() };
    let node_ids: Vec<_> = model.nodes().keys().copied().collect();
    let points_m: Vec<_> = model.nodes().values().map(|node| node.xyz).collect();
    let mut element_ids = Vec::new();
    let mut cells = Vec::new();
    let mut faces = BTreeMap::<SideId, Face>::new();
    let mut incidence = BTreeMap::new();
    for (&label, element) in model.elements() {
        if element.kind != kind {
            return Err(invalid(format!("Element {label} has the wrong type for the analysis dimension; expected {kind:?}")));
        }
        let owner = u32::try_from(cells.len()).map_err(|_| invalid("Too many solver cells"))?;
        let mut cell = Vec::with_capacity(element.nodes.len());
        let mut coordinates = Vec::with_capacity(element.nodes.len());
        for node in &element.nodes {
            let index = node_ids.binary_search(node).map_err(|_| invalid(format!("Element {label} refers to missing node {node}")))?;
            cell.push(u32::try_from(index).map_err(|_| invalid("Too many solver nodes"))?);
            coordinates.push(*points_m.get(index).ok_or_else(|| invalid("Missing mesh coordinate"))?);
        }
        verify_cell(label, kind, &coordinates)?;
        for (local, positions) in sides.iter().enumerate() {
            let nodes = positions.iter().map(|i| cell.get(*i).copied().ok_or_else(|| invalid("Invalid cell connectivity"))).collect::<Result<_>>()?;
            let face = Face { nodes, cell: owner };
            let side_id = (label, u8::try_from(local + 1).map_err(|_| invalid("Too many cell sides"))?);
            let (key, reversed) = connectivity(&face);
            let entry = incidence.entry(key).or_insert((None, None));
            let slot = if reversed { &mut entry.1 } else { &mut entry.0 };
            if slot.replace(side_id).is_some() {
                return Err(invalid("Non-manifold mesh or incident cells disagree on side orientation"));
            }
            faces.insert(side_id, face);
        }
        element_ids.push(label);
        cells.push(cell);
    }
    let exterior: BTreeSet<_> = incidence
        .values()
        .filter_map(|(a, b)| match (a, b) {
            (Some(id), None) | (None, Some(id)) => Some(*id),
            _ => None,
        })
        .collect();
    let material = assign(&analysis.materials, |name| Ok(model.resolve_set(name)?.elements), |id| model.elements().contains_key(&id))?;
    let material_ids = element_ids
        .iter()
        .map(|id| material.get(id).copied().ok_or_else(|| invalid(format!("Element {id} needs a material group"))))
        .collect::<Result<Vec<_>>>()?;
    let members = |name: &str| {
        let set = model.resolve_set(name)?;
        Ok(if planar {
            set.edges.into_iter().map(|edge| (edge.element, edge.edge)).collect()
        } else {
            set.faces.into_iter().map(|face| (face.element, face.face)).collect()
        })
    };
    let outer = assign(&analysis.boundaries, members, |id| exterior.contains(&id))?;
    if outer.len() != exterior.len() {
        return Err(invalid("The analysis's boundary sets must cover every exterior side"));
    }
    let inner = assign(&analysis.interfaces, members, |id| faces.contains_key(&id) && !exterior.contains(&id))?;
    let mut paired = BTreeSet::new();
    let mut boundaries = Vec::new();
    let mut interfaces = Vec::new();
    for (id, face) in faces {
        if let Some(&group) = outer.get(&id) {
            boundaries.push(Side { nodes: face.nodes, owner: face.cell, group, neighbor: None });
        } else if let Some(&group) = inner.get(&id) {
            let (key, reversed) = connectivity(&face);
            if !paired.insert(key) {
                return Err(invalid("An interface must select only one owning side"));
            }
            let neighbor = incidence
                .get(&key)
                .and_then(|(a, b)| if reversed { *a } else { *b })
                .and_then(|(label, _)| element_ids.binary_search(&label).ok())
                .and_then(|index| u32::try_from(index).ok())
                .ok_or_else(|| invalid("An interior side needs two incident cells"))?;
            interfaces.push(Side { nodes: face.nodes, owner: face.cell, group, neighbor: Some(neighbor) });
        }
    }
    Ok(Exchange {
        dimension: analysis.dimension,
        mesh_sha256,
        points_m,
        node_ids,
        element_ids,
        cells,
        material_names: &analysis.materials,
        material_ids,
        boundaries,
        boundary_names: &analysis.boundaries,
        interfaces,
        interface_names: &analysis.interfaces,
    })
}
