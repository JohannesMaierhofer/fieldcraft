//! Contour a solver-independent real nodal field on the exact original mesh.
use crate::contour::{Bands, banded, bands_color};
use crate::{DisplayMesh, Rgb, Scene, Vec3};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldFile {
    pub format: String,
    mesh_sha256: String,
    name: String,
    unit: String,
    components: usize,
    node_ids: Vec<u32>,
    values: Vec<f64>,
}

#[derive(Debug)]
pub struct NodalContour {
    pub title: String,
    pub min: (u32, f64),
    pub max: (u32, f64),
    labels: Vec<u32>,
    displacements_m: Option<Vec<[f64; 3]>>,
    values: Vec<f64>,
    pub range: [f64; 2],
}

impl FieldFile {
    /// Semantic name as supplied by the solver, before adding a display component or unit.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Physical unit supplied by the solver.
    pub fn unit(&self) -> &str {
        &self.unit
    }
    pub fn matches(&self, name: &str, unit: &str, components: usize) -> bool {
        self.name == name && self.unit == unit && self.components == components
    }

    pub fn prepare<'a>(
        self,
        nodes: impl ExactSizeIterator<Item = &'a u32>,
        mesh_sha256: &str,
        component: Option<&str>,
    ) -> Result<NodalContour, &'static str> {
        if self.format != "fieldcraft-nodal-field/1" || !matches!(self.components, 1 | 3) {
            return Err("expected fieldcraft-nodal-field/1 with one or three components");
        }
        if self.name.trim().is_empty()
            || self.name.len() > 120
            || self.unit.len() > 48
            || self.name.chars().chain(self.unit.chars()).any(char::is_control)
        {
            return Err("invalid field name or unit");
        }
        let count = self.node_ids.len();
        if count == 0
            || count != nodes.len()
            || count.checked_mul(self.components) != Some(self.values.len())
            || self.node_ids.iter().ne(nodes)
            || self.values.iter().any(|x| !x.is_finite())
        {
            return Err("expected finite values for every model node, in ascending label order");
        }
        if self.mesh_sha256 != mesh_sha256 {
            return Err("field belongs to a different mesh (mesh.bin SHA-256 mismatch)");
        }
        let displacements_m = (self.components == 3 && self.unit == "m").then(|| self.values.as_chunks::<3>().0.to_vec());
        let mut title = self.name;
        let values = if self.components == 1 {
            if component.is_some() {
                return Err("a scalar field has no component selector");
            }
            self.values
        } else {
            let selected = component.unwrap_or("magnitude");
            title.push_str(&format!(" ({selected})"));
            let component_index = match selected {
                "x" => Some(0),
                "y" => Some(1),
                "z" => Some(2),
                "magnitude" => None,
                _ => return Err("vector component must be x, y, z, or magnitude"),
            };
            let (vectors, _) = self.values.as_chunks::<3>();
            vectors.iter().map(|v| component_index.map_or_else(|| v[0].hypot(v[1]).hypot(v[2]), |i| v[i])).collect()
        };
        if !self.unit.is_empty() {
            title.push_str(&format!(" [{}]", self.unit));
        }
        let samples = self.node_ids.iter().copied().zip(values.iter().copied());
        let min = samples.clone().min_by(|a, b| a.1.total_cmp(&b.1)).ok_or("empty field")?;
        let max = samples.max_by(|a, b| a.1.total_cmp(&b.1)).ok_or("empty field")?;
        if !(max.1 - min.1).is_finite() {
            return Err("field range cannot be represented");
        }
        Ok(NodalContour { title, min, max, labels: self.node_ids, displacements_m, values, range: [min.1, max.1] })
    }
}

impl NodalContour {
    /// Only three-component displacements in SI meters can change the displayed shape.
    pub fn has_displacements(&self) -> bool {
        self.displacements_m.is_some()
    }

    /// Align a displacement with its original node label, independently of the contour component.
    pub fn displacement(&self, label: u32) -> Option<Vec3> {
        let vectors = self.displacements_m.as_ref()?;
        let index = self.labels.binary_search(&label).ok()?;
        vectors.get(index).copied().map(Vec3::from)
    }

    /// The reference display and external displacements both use SI meters.
    pub fn displaced(&self, reference: &DisplayMesh, scale: f64) -> DisplayMesh {
        reference.displaced(|label| self.displacement(label).map(|u| u * scale))
    }

    /// Reuse native band clipping and palettes in both the viewport and offscreen renders.
    pub fn draw(&self, display: &DisplayMesh, scene: &mut Scene, stops: &[Rgb], over: Rgb, none: Rgb) {
        let bands = Bands::new(self.range[0], self.range[1], 12);
        let color = |k: Option<u32>| k.map_or(none, |k| bands_color(stops, over, &bands, k));
        scene.tris.clear();
        for tri in &display.tris {
            let values = tri.nodes.map(|n| self.labels.binary_search(&n).ok().and_then(|i| self.values.get(i)).copied().unwrap_or(f64::NAN));
            banded(tri.p, values, tri.normal, &bands, &color, &mut scene.tris);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldcraft_model::{ElementKind, Model};

    fn mesh() -> Model {
        let mut s = Model::default();
        for (l, p) in [(2, [0., 0., 0.]), (9, [1., 0., 0.]), (12, [0., 1., 0.]), (23, [0., 0., 1.])] {
            s.set_node(l, p).unwrap();
        }
        s.set_element(7, ElementKind::Tet4, vec![2, 9, 12, 23]).unwrap();
        s
    }
    fn data(_s: &Model) -> FieldFile {
        FieldFile {
            format: "fieldcraft-nodal-field/1".into(),
            name: "Flux".into(),
            unit: "T".into(),
            components: 3,
            mesh_sha256: "the mesh".into(),
            node_ids: vec![2, 9, 12, 23],
            values: vec![3., 4., 0., 0., 0., 0., 0., 1., 0., 0., 0., 2.],
        }
    }
    #[test]
    fn verified_vector_components_and_contours() {
        let s = mesh();
        let magnitude = data(&s).prepare(s.nodes().keys(), "the mesh", None).unwrap();
        assert_eq!(magnitude.max, (2, 5.));
        assert_eq!(data(&s).prepare(s.nodes().keys(), "the mesh", Some("y")).unwrap().max, (2, 4.));
        assert_eq!(magnitude.title, "Flux (magnitude) [T]");
        let dm = DisplayMesh::from_model(&s);
        let mut scene = Scene::default();
        magnitude.draw(&dm, &mut scene, &[Rgb(0, 0, 0), Rgb(255, 255, 255)], Rgb(255, 0, 0), Rgb(0, 0, 0));
        assert!(scene.tris.len() > dm.tris.len());
    }
    #[test]
    fn true_and_magnified_shapes_use_all_components_and_original_labels() {
        let model = mesh();
        let base = DisplayMesh::from_model(&model);
        let mut file = data(&model);
        file.unit = "m".into();
        let field = file.prepare(model.nodes().keys(), "the mesh", Some("z")).unwrap();
        assert_eq!(field.displacement(2), Some(Vec3::new(3., 4., 0.)));
        for scale in [0.0, 0.25, 1.0, 2.5] {
            let shown = field.displaced(&base, scale);
            assert_eq!(shown.nodes[0].p, Vec3::new(3. * scale, 4. * scale, 0.));
            assert_eq!(shown.nodes[1].p, base.nodes[1].p);
            assert!(shown.tris.iter().any(|tri| tri.p.contains(&shown.nodes[0].p)));
            assert!(shown.edges.iter().any(|edge| edge.p.contains(&shown.nodes[0].p)));
        }
        assert_eq!(base.nodes[0].p, Vec3::ZERO);
        assert!(!data(&model).prepare(model.nodes().keys(), "the mesh", None).unwrap().has_displacements());
    }

    #[test]
    fn rejects_unrelated_mesh_and_invalid_data() {
        let s = mesh();
        let mut wrong = data(&s);
        wrong.mesh_sha256.clear();
        assert!(wrong.prepare(s.nodes().keys(), "the mesh", None).is_err());
        let mut mislabeled = data(&s);
        mislabeled.node_ids.swap(0, 1);
        assert!(mislabeled.prepare(s.nodes().keys(), "the mesh", None).is_err());
        let mut partial = data(&s);
        partial.values.pop();
        assert!(partial.prepare(s.nodes().keys(), "the mesh", None).is_err());
    }
}
