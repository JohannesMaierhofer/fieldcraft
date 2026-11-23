# Export a mesh to Netgen or NGSolve

Choose **Mesh → Verify → Export Netgen Mesh** and save a `.vol` file. The exporter writes
counterclockwise linear Tri3 and Quad4 meshes in the XY plane at z = 0 and linear Tet4 and Hex8
meshes in 3D.
Coordinates are in meters, regardless of the model's display units. The file carries the
existing connectivity and region names. Define material properties, boundary conditions, and
loads in your analysis script. See the [VOL format description](../formats/vol.md).

- **Material sets** assign one named region to each element. When supplied, the sets
  must cover every element. When omitted, the exporter uses section material names, CAD body
  names for unassigned elements, and `default` for untagged elements.
- **Boundary sets** name 2D edges or 3D faces, on the outside or at an interior interface.
  Select a shared edge or face from either side or both sides of the same set. The file stores it
  once with both adjacent domains. In 2D, a name used across multiple domain pairs keeps that
  name on a distinct segment index for each pair. Separate meshes retain their separate nodes.
  Other exterior boundaries and interfaces between material regions use their qualified CAD names when
  available or fall back to `default` and `interface`, respectively.
  The exporter joins the CAD body and entity names with a period and replaces whitespace, control
  characters, and `#` with underscores. If different boundaries produce the same VOL name,
  assign distinct boundary sets.

Open the result as a mesh in Netgen or load it from your existing NGSolve script:

```python
from ngsolve import Mesh

mesh = Mesh("team13.vol")
print(mesh.GetMaterials())
print(mesh.GetBoundaries())
```

## TEAM 13 nonlinear magnetostatics

The TEAM 13 benchmark places a coil around a steel plate between two steel channels.
The [TEAM 13 mesh recipe](netgen-team13.json) builds the workshop's conforming tetrahedral mesh
from its mapped blocks. It creates ten material sets and names the air box `outer`, the steel
and winding interfaces with air `gamma_core` and `gamma_coil`, and the interfaces between
winding segments `winding_seams`. [TEAM 13 benchmark description](https://aces-society.org/includes/downloadpaper.php?nf=d244321ad5d8d4bf402cdd85ec419b6c&of=1993W12)

Run the recipe to build and export `team13.vol`.

```sh
fieldcraft-cli run docs/recipes/netgen-team13.json
```

The full recipe rebuilds 3,600 blocks. To export an already-open TEAM 13 mesh, use
**Export Netgen Mesh** with the material and boundary sets in the recipe's final command.

## Planar MEMS electrostatic actuator

The [MEMS pull-in mesh recipe](netgen-mems-pull-in.json) triangulates the air gap and silicon
beam as separate XY regions. It exports material sets `air` and `beam` and boundary sets
`ground`, `outer`, `beam_electrode`, `tip`, `beam_top`, and `clamp`. Both sides of
`beam_electrode` retain the same label and their own mesh nodes.

Run the recipe to build and export `pull-in.vol`.

```sh
fieldcraft-cli run docs/recipes/netgen-mems-pull-in.json
```
