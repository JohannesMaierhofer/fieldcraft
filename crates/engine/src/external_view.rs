//! Display externally computed point and cell arrays on the grid that supplied them.
use crate::results::{DEFAULT_BANDS, MAX_DEFORMATION_SCALE};
use crate::view::{Palette, Section, Show, Shown, Style};
pub use fieldcraft_io::external::{Cell, Field, Frame, Grid, Series};
use fieldcraft_model::ElementKind;
use fieldcraft_render::contour::{Bands, banded, bands_color};
use fieldcraft_render::{Aabb3, DisplayMesh, Scene, SceneLine, ScenePoint, SceneTri, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Association {
    Point,
    Cell,
}

impl Association {
    pub fn fields(self, frame: &Frame) -> &BTreeMap<String, Field> {
        match self {
            Self::Point => &frame.point_data,
            Self::Cell => &frame.cell_data,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Point => "Point",
            Self::Cell => "Cell",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    Value,
    Magnitude,
    X,
    Y,
    Z,
}

impl Component {
    pub fn label(self) -> &'static str {
        match self {
            Self::Value => "Value",
            Self::Magnitude => "Magnitude",
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }

    pub fn for_components(components: u8) -> &'static [Self] {
        match components {
            1 => &[Self::Value],
            2 => &[Self::Magnitude, Self::X, Self::Y],
            3 => &[Self::Magnitude, Self::X, Self::Y, Self::Z],
            _ => &[],
        }
    }

    fn sample(self, field: &Field, index: usize) -> Option<f64> {
        let width = usize::from(field.components);
        let start = index.checked_mul(width)?;
        let tuple = field.values.get(start..start.checked_add(width)?)?;
        let v = match (self, tuple) {
            (Self::Value, [v]) => *v,
            (Self::Magnitude, [x, y]) => libm::hypot(*x, *y),
            (Self::Magnitude, [x, y, z]) => libm::hypot(libm::hypot(*x, *y), *z),
            (Self::X, [x, _, ..]) => *x,
            (Self::Y, [_, y, ..]) => *y,
            (Self::Z, [_, _, z]) => *z,
            _ => return None,
        };
        v.is_finite().then_some(v)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalView {
    pub frame: usize,
    pub association: Association,
    pub field: Option<String>,
    pub component: Component,
    pub displacement: Option<String>,
    pub scale: f64,
    pub show: bool,
    #[serde(default)]
    pub animate: bool,
}

impl ExternalView {
    pub fn initial(series: &Series) -> Self {
        let mut view = Self {
            frame: 0,
            association: Association::Point,
            field: None,
            component: Component::Value,
            displacement: None,
            scale: 1.0,
            show: true,
            animate: false,
        };
        if let Some(frame) = series.frames.first() {
            view.select_default(frame);
        }
        view
    }

    /// Pick the first drawable array in point, then cell order. Preserve a displacement
    /// selection when its named point vector exists in this frame.
    pub fn select_default(&mut self, frame: &Frame) {
        self.field = None;
        self.displacement = self.displacement.take().filter(|n| frame.point_data.get(n).is_some_and(|f| matches!(f.components, 2 | 3)));
        for a in [Association::Point, Association::Cell] {
            for (name, f) in a.fields(frame) {
                if let Some(&c) = Component::for_components(f.components).first() {
                    self.association = a;
                    self.field = Some(name.clone());
                    self.component = c;
                    return;
                }
            }
        }
    }

    pub fn validate(&self, series: &Series) -> Result<(), String> {
        let frame = series.frames.get(self.frame).ok_or_else(|| format!("Frame {} is outside the imported series", self.frame))?;
        if !(0.0..=MAX_DEFORMATION_SCALE).contains(&self.scale) {
            return Err(format!("Deformation scale must be finite and between 0 and {MAX_DEFORMATION_SCALE}"));
        }
        if self.animate {
            if series.frames.len() < 2 {
                return Err("Playback needs at least two imported frames".into());
            }
            for f in &series.frames {
                self.validate_frame(f)?;
            }
        } else {
            self.validate_frame(frame)?;
        }
        Ok(())
    }

    fn validate_frame(&self, frame: &Frame) -> Result<(), String> {
        if let Some(name) = &self.field {
            let f = self
                .association
                .fields(frame)
                .get(name)
                .ok_or_else(|| format!("No {} field named {name} at time {}", self.association.label(), frame.time))?;
            if !Component::for_components(f.components).contains(&self.component) {
                return Err(format!("{} is unavailable for field {name} with {} components", self.component.label(), f.components));
            }
        }
        if let Some(n) = &self.displacement {
            let f = frame
                .point_data
                .get(n)
                .filter(|f| matches!(f.components, 2 | 3))
                .ok_or_else(|| format!("Displacement {n} must be a two- or three-component point field"))?;
            if !frame
                .grid
                .points
                .iter()
                .zip(f.values.chunks_exact(usize::from(f.components)))
                .all(|(xyz, v)| xyz.iter().zip(v).all(|(p, d)| (p + self.scale * d).abs() <= fieldcraft_model::MAX_COORD))
            {
                return Err("Deformed coordinates exceed the model coordinate limit".into());
            }
        }
        Ok(())
    }

    /// The contour scale stays fixed as the user scrubs or plays a time series. A frame
    /// missing the selected array or component contributes no samples.
    pub fn bands(&self, series: &Series) -> Option<Bands> {
        let mut limits: Option<(f64, f64)> = None;
        for frame in &series.frames {
            if let Some(field) = self.field(frame).filter(|f| Component::for_components(f.components).contains(&self.component)) {
                for i in 0..field.values.len() / usize::from(field.components).max(1) {
                    if let Some(x) = self.component.sample(field, i) {
                        let (lo, hi) = limits.get_or_insert((x, x));
                        *lo = lo.min(x);
                        *hi = hi.max(x);
                    }
                }
            }
        }
        limits.map(|(lo, hi)| Bands::new(lo, hi, DEFAULT_BANDS))
    }

    /// Hold each frame until the following recorded time is reached during playback.
    /// Fractions use normalized times to avoid overflowing on widely separated time values.
    pub fn playback_frame(series: &Series, start: usize, fraction: f64) -> Option<usize> {
        let first = series.frames.first()?.time;
        let last = series.frames.last()?.time;
        let start = series.frames.get(start)?.time;
        if last <= first || !fraction.is_finite() {
            return None;
        }
        let factor = if (last - first).is_finite() { 1.0 } else { 0.5 };
        let lo = first * factor;
        let span = last * factor - lo;
        let normalized = |time: f64| (time * factor - lo) / span;
        let position = (normalized(start) + fraction.max(0.)).min(1.);
        Some(series.frames.partition_point(|f| normalized(f.time) <= position).saturating_sub(1))
    }
    fn field<'a>(&self, frame: &'a Frame) -> Option<&'a Field> {
        self.association.fields(frame).get(self.field.as_ref()?)
    }
}

fn element(vtk: u8) -> Option<ElementKind> {
    use ElementKind::*;
    Some(match vtk {
        3 => Rod2,
        5 => Tri3,
        9 => Quad4,
        10 => Tet4,
        12 => Hex8,
        13 => Wedge6,
        14 => Pyramid5,
        21 => Rod3,
        22 => Tri6,
        23 => Quad8,
        24 => Tet10,
        25 => Hex20,
        _ => return None,
    })
}

// Reserve at most half a frame budget for extracted topology and half for scene primitives.
// The source grid stays available if its surface representation will not fit.
const DISPLAY_BYTES: usize = fieldcraft_io::external::MAX_FRAME_BYTES / 2;

fn limited(xyz: &[[f64; 3]], palette: &Palette, reason: &str) -> (Scene, DisplayMesh, Shown) {
    let mut drawn = DisplayMesh::default();
    for &p in xyz {
        drawn.bounds.add(Vec3::from(p));
    }
    let note = format!("External results exceed the {} MiB display limit ({reason}).", DISPLAY_BYTES >> 20);
    let mut scene = Scene { background: Some(palette.background), ..Default::default() };
    scene.labels.push(fieldcraft_render::SceneLabel {
        p: drawn.bounds.center(),
        text: "External result exceeds display limit".into(),
        color: palette.marked,
    });
    (scene, drawn, Shown { failed: vec![note], ..Default::default() })
}

/// Maximum number of triangles from parallel band cuts across one original triangle.
/// A convex polygon from clipping a triangle to one slab has at most five vertices.
fn triangle_count(values: [f64; 3], bands: &Bands) -> usize {
    let [Some(a), Some(b), Some(c)] = values.map(|x| bands.band(x)) else { return 1 };
    let spread = a.max(b).max(c) - a.min(b).min(c);
    if spread == 0 { 1 } else { 3 * (spread as usize + 1) }
}

/// Split a line element at its interpolated contour levels, using the same flat band
/// colors as neighboring shell and solid elements.
fn colored_line(line: &fieldcraft_render::DisplayLine, values: [Option<f64>; 2], bands: &Bands, p: &Palette, scene: &mut Scene) {
    let push = |scene: &mut Scene, a, b, v: Option<f64>| {
        let color = v.and_then(|x| bands.band(x)).map_or(p.no_result, |k| bands_color(&p.contour, p.contour_over, bands, k));
        scene.lines.push(SceneLine { a, b, color, width: crate::view::LINE_ELEMENT_WIDTH });
    };
    let [Some(first), Some(last)] = values else {
        push(scene, line.a, line.b, None);
        return;
    };
    if bands.band(first) == bands.band(last) {
        push(scene, line.a, line.b, Some(first));
        return;
    }
    let (a, b, lo, hi) = if first <= last { (line.a, line.b, first, last) } else { (line.b, line.a, last, first) };
    let (mut start, mut base) = (a, lo);
    for level in (1..bands.count).map(|k| bands.level(k)).filter(|&v| lo < v && v < hi) {
        let end = a + (b - a) * ((level - lo) / (hi - lo));
        push(scene, start, end, Some(base + (level - base) * 0.5));
        (start, base) = (end, level);
    }
    push(scene, start, b, Some(base + (hi - base) * 0.5));
}

/// Build the external picture in its source coordinates. Vertex cells remain visible in
/// their own right, line cells carry their endpoint or cell colors, and each volume's
/// faces share the model renderer's boundary extraction and higher-order tessellation.
pub fn scene(
    frame: &Frame,
    view: &ExternalView,
    style: Style,
    show: Show,
    palette: &Palette,
    bands: Option<Bands>,
    section: Option<(Section, Vec3)>,
) -> (Scene, DisplayMesh, Shown) {
    let vectors = view.displacement.as_ref().and_then(|n| frame.point_data.get(n));
    let mut mesh_budget = DISPLAY_BYTES;
    let mut moved = Vec::new();
    let xyz = if let Some(field) = vectors {
        let count = frame.grid.points.len();
        let Some(left) = mesh_budget.checked_sub(count.saturating_mul(size_of::<[f64; 3]>())) else {
            return limited(&frame.grid.points, palette, "deformation coordinates");
        };
        if moved.try_reserve_exact(count).is_err() {
            return limited(&frame.grid.points, palette, "deformation coordinates");
        }
        mesh_budget = left;
        moved.extend(frame.grid.points.iter().zip(field.values.chunks_exact(usize::from(field.components).max(1))).map(|(&p, v)| {
            let [x, y, ..] = v else { return p };
            let z = v.get(2).copied().unwrap_or(0.0);
            [p[0] + view.scale * x, p[1] + view.scale * y, p[2] + view.scale * z]
        }));
        moved.as_slice()
    } else {
        &frame.grid.points
    };
    // Cut by reference-coordinate cell centroid, exactly as for native mesh sections.
    // Keep the original numbering for both arrays, including when hidden cells are interleaved.
    let cut = section.map(|(cut, toward)| {
        let (at, sign) = cut.cut(frame.grid.points.iter().filter_map(|p| p.get(cut.axis).copied()), toward);
        (cut.axis, at, sign)
    });
    let visible = |cell: &Cell| {
        cut.is_none_or(|(axis, at, sign)| {
            let sum: f64 = cell.points.iter().filter_map(|&n| frame.grid.points.get(n as usize)?.get(axis)).sum();
            (sum / (cell.points.len().max(1) as f64) - at) * sign <= 0.0
        })
    };
    let cells = frame.grid.cells.iter().enumerate().filter(|(_, c)| visible(c));
    let midpoints = cells.clone().filter(|(_, c)| c.vtk_type == 21).count();
    let scratch = midpoints.saturating_mul(size_of::<fieldcraft_render::DisplayLine>()).saturating_add(if cut.is_some() { xyz.len() } else { 0 });
    let Some(mesh_budget) = mesh_budget.checked_sub(scratch) else {
        return limited(xyz, palette, "section or quadratic line elements");
    };
    let elements = cells.clone().filter_map(|(id, c)| Some((u32::try_from(id).ok()?, element(c.vtk_type)?, c.points.as_slice())));
    let mut drawn = match DisplayMesh::from_indexed(xyz, elements, mesh_budget) {
        Ok(mesh) => mesh,
        Err(reason) => return limited(xyz, palette, reason),
    };
    if cut.is_some() {
        let mut kept = vec![false; xyz.len()];
        for &n in cells.clone().flat_map(|(_, c)| &c.points) {
            if let Some(value) = kept.get_mut(n as usize) {
                *value = true;
            }
        }
        drawn.nodes.retain(|n| kept.get(n.label as usize) == Some(&true));
        drawn.bounds = Aabb3::EMPTY;
        for node in &drawn.nodes {
            drawn.bounds.add(node.p);
        }
    }
    // A quadratic line passes through its middle node in VTK (end, end, middle).
    if drawn.lines.try_reserve_exact(midpoints).is_err() {
        return limited(xyz, palette, "quadratic line elements");
    }
    for i in 0..drawn.lines.len() {
        if let Some(l) = drawn.lines.get_mut(i).filter(|l| l.kind == ElementKind::Rod3)
            && let Some(&mid) = frame.grid.cells.get(l.element as usize).and_then(|c| c.points.get(2))
            && let Some(&p) = xyz.get(mid as usize)
        {
            let mid_p = Vec3::from(p);
            let end = fieldcraft_render::DisplayLine { a: mid_p, nodes: [mid, l.nodes[1]], ..*l };
            l.b = mid_p;
            l.nodes[1] = mid;
            drawn.lines.push(end);
        }
    }

    let mut shown = Shown { scale: if vectors.is_some() { view.scale } else { 0.0 }, ..Default::default() };
    let field = view.field(frame);
    let sample = |id: usize| field.and_then(|f| view.component.sample(f, id));
    let total = match view.association {
        Association::Point => xyz.len(),
        Association::Cell => frame.grid.cells.len(),
    };
    let mut range: Option<((u32, f64), (u32, f64))> = None;
    if field.is_some() {
        for id in 0..total {
            if let (Ok(i), Some(v)) = (u32::try_from(id), sample(id)) {
                let (min, max) = range.get_or_insert(((i, v), (i, v)));
                if v < min.1 {
                    *min = (i, v);
                }
                if v > max.1 {
                    *max = (i, v);
                }
            }
        }
    }
    let bands = bands.or_else(|| range.map(|(min, max)| Bands::new(min.1, max.1, DEFAULT_BANDS)));
    if let (Some((min, max)), Some(name)) = (range, view.field.as_ref()) {
        shown.contour = Some((name.clone(), min, max));
    }
    // In an externally scaled quantity, max − min or a band boundary can overflow even
    // when all individual samples are finite. Normalize only those extreme ranges.
    let factor = bands.map_or(1.0, |b| {
        let size = b.max.abs().max(b.min.abs());
        if size > f64::MAX / (2.0 * f64::from(b.count)) { 1.0 / size } else { 1.0 }
    });
    let bands = bands.map(|b| Bands::new(b.min * factor, b.max * factor, b.count));
    let value = |point: u32, cell: usize| sample(if view.association == Association::Point { point as usize } else { cell }).map(|v| v * factor);
    let color =
        |v: Option<f64>, plain| bands.and_then(|b| Some(bands_color(&palette.contour, palette.contour_over, &b, b.band(v?)?))).unwrap_or(plain);
    let mut lines = if style == Style::Shaded { 0 } else { drawn.edges.len() };
    for line in &drawn.lines {
        let segments = bands.map_or(1, |b| {
            let [a, c] = line.nodes.map(|id| value(id, line.element as usize).and_then(|v| b.band(v)));
            a.zip(c).map_or(1, |(a, c)| a.abs_diff(c) as usize + 1)
        });
        lines = lines.saturating_add(segments);
    }
    let mut points = cells.clone().filter(|(_, c)| c.vtk_type == 1).count();
    if show.nodes {
        points = points.saturating_add(drawn.nodes.len());
    }
    let fixed = lines.saturating_mul(size_of::<SceneLine>()).saturating_add(points.saturating_mul(size_of::<ScenePoint>()));
    let mut triangles = 0usize;
    if style != Style::Wireframe {
        for tri in &drawn.tris {
            let pieces = bands.as_ref().map_or(1, |b| triangle_count(tri.nodes.map(|id| value(id, tri.element as usize).unwrap_or(f64::NAN)), b));
            triangles = triangles.saturating_add(pieces);
            if triangles > DISPLAY_BYTES.saturating_sub(fixed) / size_of::<SceneTri>() {
                break;
            }
        }
    }
    if fixed > DISPLAY_BYTES || triangles > DISPLAY_BYTES.saturating_sub(fixed) / size_of::<SceneTri>() {
        return limited(xyz, palette, "contour or edge primitives");
    }
    let mut scene = Scene { background: Some(palette.background), ..Default::default() };
    // Reserve exact capacities before drawing so geometric Vec growth cannot overshoot the bound.
    if scene.lines.try_reserve_exact(lines).is_err()
        || scene.tris.try_reserve_exact(triangles).is_err()
        || scene.points.try_reserve_exact(points).is_err()
    {
        return limited(xyz, palette, "scene allocation");
    }
    if style != Style::Shaded {
        scene.lines.extend(crate::view::mesh_outlines(&drawn, style, palette));
    }
    if style != Style::Wireframe {
        for tri in &drawn.tris {
            if let Some(b) = bands {
                let band_color = |k: Option<u32>| k.map_or(palette.no_result, |k| bands_color(&palette.contour, palette.contour_over, &b, k));
                let values = tri.nodes.map(|n| value(n, tri.element as usize).unwrap_or(f64::NAN));
                banded(tri.p, values, tri.normal, &b, &band_color, &mut scene.tris);
            } else {
                scene.tris.push(SceneTri { p: tri.p, normal: tri.normal, color: palette.element(tri.kind) });
            }
        }
    }
    for line in &drawn.lines {
        if let Some(b) = bands {
            colored_line(line, line.nodes.map(|n| value(n, line.element as usize)), &b, palette, &mut scene);
        } else {
            scene.lines.push(SceneLine { a: line.a, b: line.b, color: palette.element(line.kind), width: crate::view::LINE_ELEMENT_WIDTH });
        }
    }
    for (id, cell) in cells.filter(|(_, c)| c.vtk_type == 1) {
        if let Some(&n) = cell.points.first()
            && let Some(&p) = xyz.get(n as usize)
        {
            scene.points.push(ScenePoint { p: Vec3::from(p), color: color(value(n, id), palette.node), size: 5.0 });
        }
    }
    if show.nodes {
        scene.points.extend(drawn.nodes.iter().map(|n| ScenePoint { p: n.p, color: palette.node, size: 3.0 }));
    }
    (scene, drawn, shown)
}

/// The independently imported frame currently taking the place of the model in the viewport.
pub fn visible(s: &crate::Session) -> Option<(&Frame, &ExternalView)> {
    let view = s.external_view.as_ref().filter(|v| v.show)?;
    Some((s.external_results()?.frames.get(view.frame)?, view))
}

/// Return focus to the model or its solved steps, retaining the imported data and selection.
pub(crate) fn hide(s: &mut crate::Session) {
    if let Some(v) = &mut s.external_view {
        v.show = false;
        v.animate = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Series {
        let displacements = Field { components: 2, values: vec![1.0, 0.0, 2.0, 0.0, 0.0, 1.0] };
        let grid = Grid { points: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]], cells: vec![Cell { vtk_type: 5, points: vec![0, 1, 2] }] };
        let frame = Frame {
            time: 0.,
            grid,
            point_data: BTreeMap::from([("u".into(), displacements)]),
            cell_data: BTreeMap::from([("u".into(), Field { components: 1, values: vec![13.] })]),
        };
        Series { frames: vec![frame] }
    }

    #[test]
    fn point_vectors_move_only_the_view_and_cell_colors_stay_cell_values() {
        let series = fixture();
        let frame = &series.frames[0];
        let mut view = ExternalView::initial(&series);
        assert_eq!(view.component, Component::Magnitude);
        view.association = Association::Cell;
        view.component = Component::Value;
        view.displacement = Some("u".into());
        view.scale = 0.5;
        view.validate(&series).unwrap();
        let (s, b, shown) = scene(frame, &view, Style::Shaded, Show::DEFAULT, &Palette::LIGHT, None, None);
        assert_eq!(b.bounds.max, Vec3::new(2.0, 1.5, 0.0));
        assert_eq!(s.tris.len(), 1);
        assert_eq!(s.tris[0].p, [Vec3::new(0.5, 0., 0.), Vec3::new(2., 0., 0.), Vec3::new(0., 1.5, 0.)]);
        assert_eq!(shown.contour, Some(("u".into(), (0, 13.), (0, 13.))));
        assert_eq!(frame.grid.points[1], [1., 0., 0.]);
        view.scale = 0.;
        let (_, ref_bounds, _) = scene(frame, &view, Style::Shaded, Show::DEFAULT, &Palette::LIGHT, None, None);
        assert_eq!(ref_bounds.bounds.max, Vec3::new(1., 1., 0.));
        view.displacement = Some("missing".into());
        assert!(view.validate(&series).is_err());
    }
    #[test]
    fn a_section_reveals_the_shared_face_and_keeps_original_cell_indices() {
        let grid = Grid {
            points: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., -1.]],
            cells: vec![Cell { vtk_type: 10, points: vec![0, 1, 2, 3] }, Cell { vtk_type: 10, points: vec![0, 2, 1, 4] }],
        };
        let frame = Frame {
            grid,
            time: 0.,
            point_data: Default::default(),
            cell_data: BTreeMap::from([("T".into(), Field { components: 1, values: vec![5., 30.] })]),
        };
        let series = Series { frames: vec![frame] };
        let view = ExternalView::initial(&series);
        let frame = &series.frames[0];
        let (_, all, _) = scene(frame, &view, Style::Shaded, Show::DEFAULT, &Palette::LIGHT, None, None);
        assert_eq!(all.tris.len(), 6);
        let cut = Some((Section { axis: 2, at: Some(0.0) }, Vec3::Z));
        let (colored, sliced, _) = scene(frame, &view, Style::Shaded, Show::DEFAULT, &Palette::LIGHT, None, cut);
        assert_eq!(sliced.tris.len(), 4);
        assert!(sliced.tris.iter().all(|t| t.element == 1));
        assert_eq!(sliced.bounds.max.z, 0.0);
        assert_eq!(sliced.bounds.min.z, -1.0);
        assert!(
            colored.tris.iter().all(|t| t.color == fieldcraft_render::contour::band_color(&Palette::LIGHT.contour, DEFAULT_BANDS - 1, DEFAULT_BANDS))
        );
    }

    #[test]
    fn playback_follows_irregular_times_and_contours_use_one_scale() {
        let mut series = fixture();
        for (time, pressure) in [(1.0, 17.0), (4.0, 43.0)] {
            let mut next = series.frames[0].clone();
            next.time = time;
            next.cell_data.get_mut("u").unwrap().values[0] = pressure;
            series.frames.push(next);
        }
        let mut view = ExternalView::initial(&series);
        view.association = Association::Cell;
        view.component = Component::Value;
        let bands = view.bands(&series).unwrap();
        assert_eq!((bands.min, bands.max), (13., 43.));
        for (frame, fraction) in [(0, 0.), (1, 0.25), (1, 0.5), (1, 0.99), (2, 1.)] {
            assert_eq!(ExternalView::playback_frame(&series, 0, fraction), Some(frame));
        }
        assert_eq!(ExternalView::playback_frame(&series, 1, 0.75), Some(2));
        let (start, _, _) = scene(&series.frames[0], &view, Style::Shaded, Show::DEFAULT, &Palette::LIGHT, Some(bands), None);
        let (end, _, _) = scene(&series.frames[2], &view, Style::Shaded, Show::DEFAULT, &Palette::LIGHT, Some(bands), None);
        assert_ne!(start.tris[0].color, end.tris[0].color);
        view.animate = true;
        series.frames[2].cell_data.clear();
        assert!(view.validate(&series).is_err());
    }

    #[test]
    fn line_contours_follow_every_band_at_extreme_field_values() {
        let grid = Grid { points: vec![[0., 0., 0.], [1., 0., 0.]], cells: vec![Cell { vtk_type: 3, points: vec![0, 1] }] };
        let frame = Frame {
            grid,
            time: 0.,
            point_data: BTreeMap::from([("T".into(), Field { components: 1, values: vec![-f64::MAX, f64::MAX] })]),
            cell_data: BTreeMap::new(),
        };
        let series = Series { frames: vec![frame] };
        let view = ExternalView::initial(&series);
        let (scene, _, shown) = scene(&series.frames[0], &view, Style::Shaded, Show::DEFAULT, &Palette::LIGHT, view.bands(&series), None);
        assert!(shown.failed.is_empty());
        assert_eq!(scene.lines.len(), DEFAULT_BANDS as usize);
        for (k, l) in scene.lines.iter().enumerate() {
            assert!((l.a.x - k as f64 / f64::from(DEFAULT_BANDS)).abs() < 1e-12);
            assert_eq!(l.color, fieldcraft_render::contour::band_color(&Palette::LIGHT.contour, k as u32, DEFAULT_BANDS));
        }
    }

    #[test]
    fn quadratic_line_draws_both_segments_through_the_source_midpoint() {
        let points = vec![[0., 0., 0.], [2., 0., 0.], [1., 1., 0.]];
        let grid = Grid { points, cells: vec![Cell { vtk_type: 21, points: vec![0, 1, 2] }] };
        let frame = Frame { grid, time: 0., point_data: Default::default(), cell_data: Default::default() };
        let series = Series { frames: vec![frame] };
        let view = ExternalView::initial(&series);
        let (scene, mesh, shown) = scene(&series.frames[0], &view, Style::Shaded, Show::DEFAULT, &Palette::LIGHT, None, None);
        assert!(shown.failed.is_empty());
        assert_eq!(mesh.lines.len(), 2);
        assert_eq!(scene.lines.len(), 2);
        assert_eq!(mesh.lines[0].nodes, [0, 2]);
        assert_eq!(mesh.lines[1].nodes, [2, 1]);
        assert_eq!(scene.lines[0].b, Vec3::new(1., 1., 0.));
        assert_eq!(scene.lines[0].b, scene.lines[1].a);
    }

    #[test]
    fn invalid_scales_and_excessive_displacements_are_refused() {
        let mut series = fixture();
        let mut view = ExternalView::initial(&series);
        view.displacement = Some("u".into());
        for invalid in [-1., f64::NAN, f64::INFINITY, MAX_DEFORMATION_SCALE * 2.0] {
            view.scale = invalid;
            assert!(view.validate(&series).is_err());
        }
        view.scale = 2.;
        series.frames[0].point_data.get_mut("u").unwrap().values[0] = f64::MAX;
        assert!(view.validate(&series).is_err());
    }

    #[test]
    fn vector_components_color_point_contours_and_distinguish_cell_values() {
        let series = fixture();
        let mut view = ExternalView::initial(&series);
        view.component = Component::X;
        let (scene, _, shown) = scene(&series.frames[0], &view, Style::Shaded, Show::DEFAULT, &Palette::LIGHT, None, None);
        assert_eq!(shown.contour, Some(("u".into(), (2, 0.), (1, 2.))));
        let colors: std::collections::BTreeSet<_> = scene.tris.iter().map(|t| (t.color.0, t.color.1, t.color.2)).collect();
        assert_eq!(colors.len(), DEFAULT_BANDS as usize);
        view.component = Component::Z;
        assert!(view.validate(&series).is_err());
    }
}
