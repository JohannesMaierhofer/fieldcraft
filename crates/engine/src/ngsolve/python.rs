//! The compatibility API inside the solver interpreter. FieldCraft supplies resolved mesh data.

pub(super) const HELPERS: &str = r#"
import json
from pathlib import Path
import sys
import types


def load_mesh(path):
    import netgen.meshing as nm
    import ngsolve as ng
    import numpy as np

    with np.load(path, allow_pickle=False) as archive:
        data = {key: archive[key] for key in archive.files}
    planar = 'tetrahedra' not in data
    cells = data['triangles' if planar else 'tetrahedra']
    native = nm.Mesh(dim=2 if planar else 3)
    native.AddPoints(np.ascontiguousarray(data['points_m'], dtype=np.float64))
    for tag, name in enumerate(data['material_names'], 1):
        region = cells[data['material_ids'] == tag]
        if planar:
            index = native.AddRegion(str(name), dim=2)
        else:
            index = tag
            native.SetMaterial(tag, str(name))
            region = region[:, [1, 0, 2, 3]]
        native.AddElements(2 if planar else 3, index, np.ascontiguousarray(region, dtype=np.int32), base=0)

    if planar:
        for tag, name in enumerate(data['boundary_names'], 1):
            index = native.AddRegion(str(name), dim=1)
            for a, b in data['segments'][data['boundary_ids'] == tag]:
                native.Add(nm.Element1D([nm.PointId(int(a) + 1), nm.PointId(int(b) + 1)], index=index))
    else:
        outer = data['triangles']
        materials = data['material_ids']
        owners = data['boundary_owner']
        sides = [(data['boundary_names'], data['boundary_ids'], outer, materials[owners], np.zeros(len(outer), dtype=np.int32))]
        if 'interface_names' in data:
            sides.append((data['interface_names'], data['interface_ids'], data['interface_triangles'],
                          materials[data['interface_owner']], materials[data['interface_neighbor']]))
        number = 0
        for names, ids, triangles, inside, outside in sides:
            for tag, name in enumerate(names, 1):
                group = ids == tag
                for a, b in sorted(set(zip(inside[group].tolist(), outside[group].tolist()))):
                    number += 1
                    desc = native.Add(nm.FaceDescriptor(surfnr=number, domin=a, domout=b, bc=number))
                    native.SetBCName(desc - 1, str(name))
                    face = triangles[group & (inside == a) & (outside == b)]
                    native.AddElements(2, desc, np.ascontiguousarray(face, dtype=np.int32), base=0)
    native.UpdateTopology()
    mesh = ng.Mesh(native)
    if mesh.ne != len(cells) or mesh.nv != len(data['node_ids']):
        raise ValueError('The solver mesh changed the vertex or element count')
    if set(mesh.GetMaterials()) != set(data['material_names']) or set(mesh.GetBoundaries()) != set(data['boundary_names']) | set(data.get('interface_names', [])):
        raise ValueError('The solver mesh lost a named region')
    return mesh, data


def _save_json(path, value):
    with path.open('x', encoding='utf-8') as stream:
        json.dump(value, stream, allow_nan=False, separators=(',', ':'))
        stream.write('\n')


def _write_field(path, data, name, unit, values):
    import numpy as np
    nodes = data['node_ids']
    values = np.asarray(values)
    if values.shape not in ((len(nodes),), (len(nodes), 3)) or np.iscomplexobj(values) or not np.isfinite(values).all():
        raise ValueError(f'Expected one finite real scalar or vector per node for {name!r}')
    components = 1 if values.ndim == 1 else 3
    _save_json(path, dict(format='fieldcraft-nodal-field/1', mesh_sha256=str(data['mesh_sha256'].item()),
                          name=name, unit=unit, components=components, node_ids=nodes.tolist(),
                          values=values.astype(float, copy=False).ravel().tolist()))
    return components


def write_results(output, data, fields, integrals, *, series=None, loads=None):
    import math
    out = Path(output)
    out.mkdir(parents=True, exist_ok=True)
    manifest = dict(mesh_sha256=str(data['mesh_sha256'].item()), fields=[], integrals=integrals, series=[])
    if loads is not None:
        manifest['loads'] = loads
    for index, (name, (unit, values)) in enumerate(fields.items(), 1):
        file = f'field-{index}.json'
        _write_field(out / file, data, name, unit, values)
        manifest['fields'].append(dict(name=name, unit=unit, file=file))
    for index, (name, (unit, samples)) in enumerate((series or {}).items(), 1):
        frames = []
        components = None
        previous = -math.inf
        for frame, (seconds, values) in enumerate(samples, 1):
            seconds = float(seconds)
            if frame > 4096 or not math.isfinite(seconds) or seconds <= previous:
                raise ValueError(f'Series {name!r} needs up to 4096 strictly increasing finite times')
            previous = seconds
            file = f'series-{index}-frame-{frame:04d}.json'
            written = _write_field(out / file, data, name, unit, values)
            if components is not None and components != written:
                raise ValueError(f'Series {name!r} changed its number of components')
            components = written
            frames.append(dict(time=seconds, file=file))
        if len(frames) < 2:
            raise ValueError(f'Series {name!r} needs at least two frames')
        file = f'series-{index}.json'
        _save_json(out / file, dict(format='fieldcraft-nodal-series/1', mesh_sha256=manifest['mesh_sha256'],
                                    name=name, unit=unit, components=components, time_unit='s', frames=frames))
        manifest['series'].append(dict(name=name, unit=unit, file=file))
    _save_json(out / 'results.json', manifest)


_fc_results = types.ModuleType('fc_results')
_fc_results.load_mesh = load_mesh
_fc_results.write_results = write_results
sys.modules['fc_results'] = _fc_results
"#;

pub(super) const ANALYSIS: &str = r#"
import platform
import runpy
import netgen
import ngsolve
import numpy as np

script, output = map(Path, sys.argv[1:])
with (output / 'mesh.json').open(encoding='utf-8') as stream:
    exchange = json.load(stream)
planar = exchange['dimension'] == 2
arrays = {name: np.asarray(exchange[name]) for name in ('mesh_sha256', 'points_m', 'node_ids', 'element_ids', 'material_names', 'material_ids', 'boundary_names')}
arrays['triangles' if planar else 'tetrahedra'] = np.asarray(exchange['cells'], dtype=np.int32)
boundaries = exchange['boundaries']
arrays['segments' if planar else 'triangles'] = np.asarray([face['nodes'] for face in boundaries], dtype=np.int32)
arrays['boundary_ids'] = np.asarray([face['group'] for face in boundaries], dtype=np.int32)
arrays['boundary_owner'] = np.asarray([face['owner'] for face in boundaries], dtype=np.int32)
interfaces = exchange['interfaces']
if interfaces:
    arrays['interface_names'] = np.asarray(exchange['interface_names'])
    for key, name in [('nodes', 'triangles'), ('group', 'ids'), ('owner', 'owner'), ('neighbor', 'neighbor')]:
        arrays['interface_' + name] = np.asarray([face[key] for face in interfaces], dtype=np.int32)
mesh_path = output / 'mesh.npz'
np.savez_compressed(mesh_path, **arrays)
del arrays, exchange, boundaries, interfaces
_save_json(output / 'runtime.json', dict(python=sys.version, ngsolve=ngsolve.__version__, netgen=netgen.__version__,
                                        numpy=np.__version__, platform=platform.platform()))
sys.argv = [str(script), str(mesh_path), str(output)]
sys.path.insert(0, str(script.parent))
runpy.run_path(str(script), run_name='__main__')
"#;

pub fn with_helpers(source: &str) -> String {
    format!("{HELPERS}\n{source}")
}
