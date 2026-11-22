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
Push-Location $out
try {
    $runtime = & $python -c "import json,sys,sysconfig,numpy,ngsolve,netgen; print(json.dumps(dict(python=sys.version,platform=sysconfig.get_platform(),numpy=numpy.__version__,ngsolve=ngsolve.__version__,netgen=netgen.__version__)))" | ConvertFrom-Json
    if ($runtime.python -notlike '3.13.18 *' -or $runtime.numpy -ne '2.3.5' -or $runtime.ngsolve -ne '6.2.2608' -or $runtime.netgen -ne '6.2.2608') { throw 'Unexpected solver runtime' }
    $runtime | ConvertTo-Json | Set-Content runtime.json
    & $cli --version
    Copy-Item "$homeDir/bridge/samples/slab.json" .
    & $cli run slab.json
    & $launcher slab.fieldcraft "$homeDir/bridge/samples/potential.py" first-run --materials dielectric --boundaries left right wall
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
        $ready = $false
        for ($n=0; $n -lt 120 -and -not $ready; $n++) {
            if ($app.HasExited) { throw "FieldCraft exited with $($app.ExitCode): $(Get-Content "$out/gui.stderr" -Raw)" }
            $socket = [Net.Sockets.TcpClient]::new()
            try { $socket.Connect('127.0.0.1',$port); $ready = $true } catch { Start-Sleep -Milliseconds 500 } finally { $socket.Dispose() }
        }
        if (-not $ready) { throw 'No GUI control listener after 60 seconds' }
        & $python "$homeDir/bridge/show_run.py" first-run --port $port --field potential --integrals --open-model
        & $python -c "import sys;sys.path.insert(0,sys.argv[1]);from show_run import Control;print(Control(int(sys.argv[2]))('ui.capture',dict(path=sys.argv[3],width=1200,height=760,scale=1)))" "$homeDir/bridge" $port "$out/window.png"
        if ((Get-Item "$out/window.png").Length -lt 1000) { throw 'Missing native window capture' }
    } finally { if (-not $app.HasExited) { $app.Kill(); $app.WaitForExit() } }
} finally {
    Pop-Location
    if ($xserver -and -not $xserver.HasExited) { $xserver.Kill() }
}
