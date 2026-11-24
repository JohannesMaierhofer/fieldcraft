[CmdletBinding()]
param([Parameter(Mandatory)][ValidateSet('x64','arm64')][string]$Arch)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = (Resolve-Path "$PSScriptRoot/../../..").Path
$setup = Join-Path $root "dist/ngsolve/FieldCraft-NGSolve-0.8.1-r4-windows-$Arch-network.exe"
$work = Join-Path $env:RUNNER_TEMP ("fieldcraft-windows-qa-" + [guid]::NewGuid().ToString('N'))
$dest = Join-Path $work 'Installed FieldCraft'
$proof = Join-Path $root 'verification'
New-Item -ItemType Directory -Force $work,$proof | Out-Null
$env:FIELDCRAFT_RECOVERY_DIR = Join-Path $work 'profile/recovery'
$env:OPENBLAS_NUM_THREADS = '1'
$env:OMP_NUM_THREADS = '1'

function Install-NetworkPackage {
    $process = Start-Process -FilePath $setup -ArgumentList "/S /NOSHORTCUTS /D=$dest" -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        $log = Get-ChildItem ([IO.Path]::GetTempPath()) -Filter 'FieldCraft-NGSolve-network-*.log' |
            Sort-Object LastWriteTime | Select-Object -Last 1
        if ($log) { Get-Content -LiteralPath $log.FullName -Tail 60 }
        throw "Installer returned $($process.ExitCode)."
    }
}

Install-NetworkPackage
$generations = @(Get-ChildItem $dest -Directory -Filter 'g-*')
if ($generations.Count -ne 1) { throw 'Expected one installed generation.' }
$app = $generations[0].FullName
$receipt = Get-Content "$app/identity.json" -Raw | ConvertFrom-Json
if ($receipt.arch -ne $Arch -or $receipt.desktop -cne $env:FIELDCRAFT_BUILD_SHA -or
    $receipt.installer_revision -ne 'r4' -or $receipt.desktop_version -ne $env:FIELDCRAFT_VERSION -or
    $generations[0].Name -cne ('g-' + $receipt.generation)) { throw 'Incorrect installed desktop identity.' }
$launcher = "$app/fieldcraft-ngsolve.cmd"
$cli = "$app/fieldcraft-cli.exe"
$gui = "$app/fieldcraft.exe"
$python = "$app/python/python.exe"

# PE machine fields verify both native binaries and the emulated solver on Windows ARM64.
$machineCheck = @'
from pathlib import Path
import struct, sys
def machine(path):
    with Path(path).open("rb") as stream:
        stream.seek(0x3c)
        offset, = struct.unpack("<I", stream.read(4))
        stream.seek(offset)
        assert stream.read(4) == b"PE\0\0"
        return struct.unpack("<H", stream.read(2))[0]
expected = {"arm64": 0xaa64, "x64": 0x8664}[sys.argv[1]]
assert [machine(path) for path in sys.argv[2:]] == [expected, expected, 0x8664]
'@
& $python -I -c $machineCheck $Arch $gui $cli $python
if ((& $cli --version) -cne "fieldcraft-cli $env:FIELDCRAFT_VERSION") { throw 'Incorrect CLI version.' }
Move-Item $launcher "$work/saved-launcher.cmd"
Install-NetworkPackage
if ((Get-FileHash $launcher).Hash -ne (Get-FileHash "$work/saved-launcher.cmd").Hash) {
    throw 'Reinstall failed to repair the installed launcher.'
}

Push-Location $work
try {
    Copy-Item "$app/examples/*" $work
    & $cli run slab.json
    & $launcher run slab.analysis.json result
    $result = Get-Content 'result/results.json' -Raw | ConvertFrom-Json
    $energy = @($result.integrals | Where-Object name -eq stored_energy)
    if ($energy.Count -ne 1 -or $energy[0].unit -ne 'J' -or
        [Math]::Abs($energy[0].value / 4.4270939064e-14 - 1) -gt 1e-8 -or $result.fields.Count -ne 2) {
        throw 'Installed capacitor example failed its energy or field count check.'
    }
    & $cli run result/view-1.json
    & $cli run result/view-2.json
    Copy-Item result/results.json "$proof/capacitor-results.json"
    Copy-Item "$app/identity.json" "$proof/identity.json"
    & "$PSScriptRoot/../verify-series.ps1" -Python $python -Cli $cli -Gui $gui -Launcher $launcher -OutputDir $proof
} finally { Pop-Location }
