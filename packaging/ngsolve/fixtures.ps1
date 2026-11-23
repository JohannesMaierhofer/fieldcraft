# Analytic fixtures for verification of the installed FieldCraft application.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Directory,
    [Parameter(Mandatory)][ValidateSet('electrostatic','diffusion')][string]$Scenario
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Directory | Out-Null
if ($Scenario -eq 'electrostatic') {
    @'
[
  {"command": "model.units", "params": {"system": "m-kg-s-N-Pa"}},
  {"command": "mesh.block", "params": {"body": "slab", "kind": "tet4", "corners": [[0, 0, 0], [1, 0, 0], [1, 0.1, 0], [0, 0.1, 0], [0, 0, 0.1], [1, 0, 0.1], [1, 0.1, 0.1], [0, 0.1, 0.1]], "divisions": [8, 2, 2], "faces": {"1": "wall", "2": "wall", "3": "wall", "4": "right", "5": "wall", "6": "left"}}},
  {"command": "model.set.create", "params": {"name": "dielectric", "geometry": [{"body": "slab"}]}},
  {"command": "model.set.create", "params": {"name": "left", "geometry": [{"body": "slab", "face": "left"}]}},
  {"command": "model.set.create", "params": {"name": "right", "geometry": [{"body": "slab", "face": "right"}]}},
  {"command": "model.set.create", "params": {"name": "wall", "geometry": [{"body": "slab", "face": "wall"}]}},
  {"command": "file.save", "params": {"path": "slab.fieldcraft"}}
]
'@ | Set-Content -Encoding utf8 (Join-Path $Directory 'slab.json')
    @'
{
  "format": "fieldcraft-ngsolve-analysis/1",
  "model": "slab.fieldcraft",
  "script": "potential.py",
  "materials": ["dielectric"],
  "boundaries": ["left", "right", "wall"],
  "field": "electric_field",
  "component": "x"
}
'@ | Set-Content -Encoding utf8 (Join-Path $Directory 'slab.analysis.json')
    @'
"""One-volt linear potential on a one-metre FieldCraft slab with left and right electrodes."""
import sys
import ngsolve as ng
from fc_results import load_mesh, write_results


def solve(source, destination):
    mesh, data = load_mesh(source)
    if not {"left", "right"} <= set(mesh.GetBoundaries()):
        raise ValueError("The slab needs left and right electrode sets")
    fes = ng.H1(mesh, order=2, dirichlet="left|right")
    u, v = fes.TnT()
    stiffness = ng.BilinearForm(ng.grad(u)*ng.grad(v)*ng.dx).Assemble()
    phi = ng.GridFunction(fes)
    electrodes = {"left": 1, "right": 0}
    phi.Set(mesh.BoundaryCF(electrodes), definedon=mesh.Boundaries("left|right"))
    residual = phi.vec.CreateVector()
    residual.data = -stiffness.mat*phi.vec
    phi.vec.data += stiffness.mat.Inverse(fes.FreeDofs(), inverse="sparsecholesky")*residual
    eps = 8.8541878128e-12
    electric = -ng.grad(phi)
    energy = ng.Integrate(0.5*eps*(electric*electric), mesh)
    nodes = mesh(*data["points_m"].T)
    fields = {"potential": ("V", phi(nodes).ravel()), "electric_field": ("V/m", electric(nodes).reshape(-1,3))}
    write_results(destination, data, fields, [{"name": "stored_energy", "unit": "J", "value": energy}],
                  loads=[f"{face} electrode ({voltage} V)" for face, voltage in electrodes.items()])


if __name__ == "__main__":
    solve(*sys.argv[1:])
'@ | Set-Content -Encoding utf8 (Join-Path $Directory 'potential.py')
} else {
    @'
[
  {"command": "model.units", "params": {"system": "m-kg-s-N-Pa"}},
  {"command": "mesh.block", "params": {"body": "slab", "kind": "tet4", "corners": [[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.01, 0.002, 0.0], [0.0, 0.002, 0.0], [0.0, 0.0, 0.002], [0.01, 0.0, 0.002], [0.01, 0.002, 0.002], [0.0, 0.002, 0.002]], "divisions": [24, 2, 2], "faces": {"1": "wall", "2": "wall", "3": "wall", "4": "right", "5": "wall", "6": "left"}}},
  {"command": "model.set.create", "params": {"name": "conductor", "geometry": [{"body": "slab"}]}},
  {"command": "model.set.create", "params": {"name": "left", "geometry": [{"body": "slab", "face": "left"}]}},
  {"command": "model.set.create", "params": {"name": "right", "geometry": [{"body": "slab", "face": "right"}]}},
  {"command": "model.set.create", "params": {"name": "wall", "geometry": [{"body": "slab", "face": "wall"}]}},
  {"command": "file.save", "params": {"path": "diffusion.fieldcraft"}}
]
'@ | Set-Content -Encoding utf8 (Join-Path $Directory 'mesh.json')
    @'
{
  "format": "fieldcraft-ngsolve-analysis/1",
  "model": "diffusion.fieldcraft",
  "script": "solve.py",
  "materials": [
    "conductor"
  ],
  "boundaries": [
    "left",
    "right",
    "wall"
  ],
  "series": "B",
  "component": "y"
}
'@ | Set-Content -Encoding utf8 (Join-Path $Directory 'slab.analysis.json')
    @'
"""Magnetoquasistatic free decay of two spatial modes in a conducting slab."""
import json
import math
from pathlib import Path
import sys

import ngsolve as ng
import numpy as np

from fc_results import load_mesh, write_results


def solve(source, destination):
    mesh, data = load_mesh(source)
    if set(mesh.GetMaterials()) != {'conductor'} or set(mesh.GetBoundaries()) != {'left','right','wall'}:
        raise ValueError('Expected one conductor with left, right and symmetry boundaries')
    length = 0.01
    mu, sigma = 1.25663706127e-6, 1.0e7
    amplitude, harmonic = 0.1, 0.3
    k = math.pi/length
    tau = mu*sigma/k**2
    dt = tau/32
    fes = ng.H1(mesh, order=2, dirichlet='left|right')
    u, v = fes.TnT()
    diffusion = ng.grad(u)*ng.grad(v)/(mu*sigma)
    lhs = ng.BilinearForm((u*v + 0.5*dt*diffusion)*ng.dx).Assemble()
    rhs = ng.BilinearForm((u*v - 0.5*dt*diffusion)*ng.dx).Assemble()
    inverse = lhs.mat.Inverse(fes.FreeDofs(), inverse='sparsecholesky')
    a = ng.GridFunction(fes)
    a.Set(amplitude/k*(ng.sin(k*ng.x) + harmonic/2*ng.sin(2*k*ng.x)))
    fixed = ~np.fromiter(fes.FreeDofs(), dtype=bool, count=fes.ndof)
    a.vec.FV().NumPy()[fixed] = 0.0
    b = ng.CF((ng.grad(a)[1], -ng.grad(a)[0], 0))
    display = ng.VectorH1(mesh, order=1)
    trial, test = display.TnT()
    projector = ng.BilinearForm((trial*test)*ng.dx).Assemble().mat.Inverse(display.FreeDofs(), inverse='sparsecholesky')
    load = ng.LinearForm((b*test)*ng.dx)
    b_nodes = ng.GridFunction(display)
    points = mesh(*data['points_m'].T)
    volume = ng.Integrate(1,mesh)
    frames, checks = [], []
    for step in range(65):
        if step % 4 == 0:
            t = step*dt
            load.Assemble()
            b_nodes.vec.data = projector*load.vec
            frames.append((t, np.asarray(b_nodes(points)).reshape(-1,3).copy()))
            exact_y = -amplitude*(ng.cos(k*ng.x)*math.exp(-t/tau)
                                  + harmonic*ng.cos(2*k*ng.x)*math.exp(-4*t/tau))
            error = b - ng.CF((0,exact_y,0))
            e_l2 = math.sqrt(ng.Integrate(error*error,mesh)/ng.Integrate(exact_y**2,mesh))
            energy = ng.Integrate(b*b/(2*mu),mesh)
            reference = volume*amplitude**2/(4*mu)*(math.exp(-2*t/tau)+harmonic**2*math.exp(-8*t/tau))
            checks.append(dict(time_s=t, energy_J=energy, exact_energy_J=reference, relative_B_L2=e_l2))
        if step < 64:
            right = a.vec.CreateVector()
            right.data = rhs.mat*a.vec
            a.vec.data = inverse*right
    if max(c['relative_B_L2'] for c in checks) > .02:
        raise ValueError('The transient field differs from the analytic reference by more than 2%')
    if not all(a['energy_J'] > b['energy_J'] for a,b in zip(checks,checks[1:])):
        raise ValueError('Magnetic energy must decrease during free decay')
    integral = [{'name':'final_magnetic_energy','unit':'J','value':checks[-1]['energy_J']}]
    write_results(destination, data, {'final_B':('T',frames[-1][1])}, integral, series={'B':('T',frames)})
    (Path(destination)/'diffusion-check.json').write_text(json.dumps({'tau_s':tau,'dt_s':dt,'frames':checks},indent=2)+'\n')


if __name__ == '__main__':
    solve(*sys.argv[1:])
'@ | Set-Content -Encoding utf8 (Join-Path $Directory 'solve.py')
}
@'
import json
from pathlib import Path
import socket
import time


def connect(port):
    def rpc(method, params=None):
        request = dict(id=1, method=method, params=params or {})
        with socket.create_connection(('127.0.0.1', port), timeout=5) as conn:
            conn.settimeout(65)
            conn.sendall(json.dumps(request).encode() + b'\n')
            with conn.makefile() as stream:
                reply = json.loads(stream.readline())
        if not reply['ok']:
            raise RuntimeError(reply['error'])
        return reply['result']

    deadline = time.monotonic() + 90
    while True:
        try:
            rpc('ui.inspect')
            return rpc
        except OSError:
            if time.monotonic() > deadline:
                raise
            time.sleep(.25)


def open_run(rpc, directory):
    manifest = (Path(directory) / 'results.json').resolve(strict=True)
    record = json.loads(manifest.read_text())
    model = str(Path(record['model']).resolve(strict=True))
    rpc('engine.execute', dict(command='file.open', params=dict(path=model)))
    return manifest, record
'@ | Set-Content -Encoding utf8 (Join-Path $Directory 'verify_ui.py')
