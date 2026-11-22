# Displaying a field from an external calculation

Save the mesh with `file.save` (`results: false` suffices). The native model package already contains
the original node and element labels, coordinates in metres, body and face tags, and model sets.
An external program can read them using the documented [package format](../formats/fieldcraft.md).
The solver runs in its own process. Keep its version, input, units and integral outputs beside its results.

To show a scalar or one vector component in the viewport, run
`results.external.contour {"path":"pressure.field.json"}` on the model that supplied the mesh.
For a vector the default is its magnitude; choose `"component":"x"`, `"y"` or `"z"` for a Cartesian component.
FieldCraft uses its standard contour bands. The command returns the displayed name and unit and the
minimum and maximum with their node labels. `fieldcraft-cli snapshot` uses the same contour after a
script opens the model and runs the command. `{"show":false}` clears the overlay. A model edit
invalidates it. Reissue the command after opening the model in a fresh session.

The UTF-8 JSON boundary is deliberately solver independent. A VTU adapter should verify the mesh
coordinates in metres, cell connectivity and original point labels before constructing the payload:

```json
{
  "format": "fieldcraft-nodal-field/1",
  "mesh_sha256": "<SHA-256 of the mesh.bin entry from fieldcraft.json>",
  "name": "pressure amplitude",
  "unit": "Pa",
  "components": 1,
  "node_ids": [2, 9, 12, 23],
  "values": [0.32, 1.28, 2.56, 0.64]
}
```

Include every mesh node once, in ascending label order. Values are finite real numbers in the
stated unit, flattened node by node. For a vector, set `components` to 3 and provide x, y, z for
each node. For harmonic solutions export separately named real, imaginary or amplitude fields.
The file limit is 64 MiB. Keep the source VTU beside this view file for access to all fields.

Use **Results → Query → External Integrals** and select the external run manifest to inspect resultant quantities. The command reply lists each name, value and unit. The same operation is available to scripts as `results.external.integrals {"path":"results.json"}`. The manifest supplies `format: "fieldcraft-external-run/1"`, `mesh_sha256`, and an `integrals` array. Each quantity carries a unique name, unit and one finite real value or three Cartesian components.

On the Results tab, **External Field** opens a nodal sidecar and selects its scalar value, vector magnitude or Cartesian component. **External Integrals** opens the matching run manifest and shows the signed scalar and vector quantities with their units in the reply panel. Both commands verify the current mesh before displaying data.
