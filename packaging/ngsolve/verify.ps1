[CmdletBinding()]
param([Parameter(Mandatory)][ValidateSet('windows-arm64','windows-x64','linux-x64')][string]$Label)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = (Get-Location).Path
$out = Join-Path $root 'verification'
New-Item -ItemType Directory $out | Out-Null
$env:PYTHONNOUSERSITE = '1'
$env:PYTHONDONTWRITEBYTECODE = '1'
$env:OPENBLAS_NUM_THREADS = '1'
$env:OMP_NUM_THREADS = '1'
if ($IsWindows) {
    $homeDir = Join-Path $env:RUNNER_TEMP 'Installed FieldCraft'
    $setup = (Get-ChildItem "$root/dist/ngsolve/*.exe").FullName
    $p = Start-Process $setup -ArgumentList "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /DIR=`"$homeDir`"" -Wait -PassThru
    if ($p.ExitCode) { throw "Installer returned $($p.ExitCode)" }
    $python = "$homeDir/python/python.exe"
    $cli = "$homeDir/fieldcraft-cli.exe"
    $gui = "$homeDir/fieldcraft.exe"
    $launcher = "$homeDir/fieldcraft-ngsolve.cmd"
    function PeMachine($path) {
        $reader = [IO.BinaryReader]::new([IO.File]::OpenRead($path))
        try {
            $reader.BaseStream.Position = 0x3c
            $offset = $reader.ReadUInt32()
            $reader.BaseStream.Position = $offset
            if ($reader.ReadUInt32() -ne 0x4550) { throw "Invalid PE at $path" }
            return $reader.ReadUInt16()
        } finally { $reader.Dispose() }
    }
    $native = if ($Label -eq 'windows-arm64') { 0xaa64 } else { 0x8664 }
    if ((PeMachine $gui) -ne $native -or (PeMachine $cli) -ne $native -or (PeMachine $python) -ne 0x8664) {
        throw 'Unexpected GUI/CLI/interpreter architecture'
    }
} else {
    $deb = (Get-ChildItem "$root/dist/ngsolve/*.deb").FullName
    sudo apt-get update -qq
    sudo apt-get install -y --no-install-recommends $deb xvfb xauth mesa-vulkan-drivers libxkbcommon-x11-0
    $homeDir = '/opt/fieldcraft-ngsolve'
    $cli = '/usr/bin/fieldcraft-ngsolve-cli'
    $gui = '/usr/bin/fieldcraft-ngsolve-gui'
    $python = "$homeDir/python/bin/python3"
    $launcher = '/usr/bin/fieldcraft-ngsolve'
    $env:DISPLAY = ':97'
    $xserver = Start-Process 'Xvfb' -ArgumentList ':97 -screen 0 1400x900x24' -PassThru
}
$distributedNotice = (Get-FileHash "$root/dist/ngsolve/SOLVER-NOTICES.txt").Hash
if ((Get-FileHash "$homeDir/SOLVER-NOTICES.txt").Hash -ne $distributedNotice) {
    throw "External solver notice differs between the installer and its download"
}
Push-Location $out
try {
    $runtime = & $python -c "import json,sys,sysconfig,numpy,ngsolve,netgen; print(json.dumps(dict(python=sys.version,platform=sysconfig.get_platform(),numpy=numpy.__version__,ngsolve=ngsolve.__version__,netgen=netgen.__version__)))" | ConvertFrom-Json
    if ($runtime.python -notlike '3.13.18 *' -or $runtime.numpy -ne '2.3.5' -or $runtime.ngsolve -ne '6.2.2608' -or $runtime.netgen -ne '6.2.2608') { throw 'Unexpected solver runtime' }
    $runtime | Add-Member -NotePropertyName host -NotePropertyValue ([Runtime.InteropServices.RuntimeInformation]::OSDescription)
    if ($IsWindows) {
        $runtime | Add-Member -NotePropertyName gui_pe_machine -NotePropertyValue ('0x{0:x4}' -f (PeMachine $gui))
        $runtime | Add-Member -NotePropertyName cli_pe_machine -NotePropertyValue ('0x{0:x4}' -f (PeMachine $cli))
        $runtime | Add-Member -NotePropertyName solver_pe_machine -NotePropertyValue ('0x{0:x4}' -f (PeMachine $python))
    }
    $runtime | ConvertTo-Json | Set-Content runtime.json
    & $cli --version
    & "$PSScriptRoot/fixtures.ps1" -Directory $out -Scenario electrostatic
    & $cli run slab.json
    & $launcher run slab.analysis.json first-run
    $m = Get-Content -Raw first-run/results.json | ConvertFrom-Json
    $e = $m.integrals | Where-Object name -eq stored_energy
    $reference = 4.4270939064e-14
    if ($e.unit -ne 'J' -or [Math]::Abs($e.value/$reference-1) -gt 1.0e-8 -or $m.fields.Count -ne 2) {
        throw 'Slab energy or field count differs from the analytic case'
    }
    Copy-Item first-run/results.json slab-results.json
    & $cli run first-run/view-1.json
    & $cli run first-run/view-2.json
    $port = 28744
    $app = Start-Process $gui -ArgumentList "--control $port" -PassThru -RedirectStandardError "$out/gui.stderr"
    try {
        $capture = @'
import json
from pathlib import Path
import sys
import time
from verify_ui import connect, open_run

control = connect(int(sys.argv[1]))
manifest, record = open_run(control, 'first-run')
out = Path(sys.argv[2]).resolve()
control('engine.execute', dict(command='results.external.integrals', params=dict(path=str(manifest))))
control('ui.move', {'x': 3, 'y': 3})
for entry, label in zip(record['fields'], ('window', 'vector'), strict=True):
    path = str(manifest.parent / entry['file'])
    params = dict(path=path, **({'component':'x'} if entry['name'] == 'electric_field' else {}))
    shown = control('engine.execute', dict(command='results.external.contour', params=params))
    assert shown['shown'], shown
    time.sleep(.35)
    state = control('ui.inspect')
    proof = {key: state.get(key) for key in ('nodes', 'elements', 'renderer', 'adapter', 'results')}
    window = out / (label + '.png')
    window.with_suffix('.json').write_text(json.dumps(proof, indent=2), encoding='utf-8')
    control('ui.capture', dict(path=str(window), width=1200, height=760, scale=1))
    assert window.stat().st_size >= 1000, 'Missing native window capture'
control('app.quit')
'@
        & $python -c $capture $port $out
        if (-not $app.WaitForExit(15000)) { throw 'FieldCraft did not close after app.quit' }
        if ($app.ExitCode -ne 0) { throw "FieldCraft exited with code $($app.ExitCode)" }
    } finally { if (-not $app.HasExited) { $app.Kill(); $app.WaitForExit() } }
    & "$PSScriptRoot/verify-series.ps1" -Python $python -Cli $cli -Gui $gui -Launcher $launcher -OutputDir $out
} finally {
    Pop-Location
    if ($xserver -and -not $xserver.HasExited) { $xserver.Kill() }
}
