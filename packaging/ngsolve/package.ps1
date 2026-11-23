[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Target,
    [Parameter(Mandatory)][ValidateSet('windows-arm64','windows-x64','linux-x64')][string]$Label
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = (Get-Location).Path
$source = Join-Path $root 'app-source'
$build = Join-Path $source "target/$Target/release"
$out = Join-Path $root 'dist/ngsolve'
$work = Join-Path ([IO.Path]::GetTempPath()) ("fieldcraft-package-" + [guid]::NewGuid().ToString('N'))
$payload = Join-Path $work 'payload'
$wheels = Join-Path $work 'wheels'
New-Item -ItemType Directory -Path $payload,$wheels,$out -Force | Out-Null
$lock = Get-Content -Raw "$PSScriptRoot/runtime-lock.json" | ConvertFrom-Json -AsHashtable
$platform = if ($IsWindows) { 'windows' } else { 'linux' }
$pins = $lock[$platform]
function Fetch($Record, [string]$Path) {
    curl --fail --location --retry 4 --silent --show-error --output $Path $Record.url
    if ((Get-FileHash -Algorithm SHA256 $Path).Hash -ne $Record.sha256) {
        throw "SHA-256 mismatch for $Path"
    }
}
Fetch $pins.runtime "$work/python.tar.gz"
tar -xzf "$work/python.tar.gz" -C $payload
$python = if ($IsWindows) { "$payload/python/python.exe" } else { "$payload/python/bin/python3" }
$env:PYTHONNOUSERSITE = '1'
$env:PYTHONDONTWRITEBYTECODE = '1'
$env:PIP_DISABLE_PIP_VERSION_CHECK = '1'
foreach ($wheel in $pins.wheels) { Fetch $wheel (Join-Path $wheels $wheel.name) }
$wheelPaths = (Get-ChildItem $wheels -Filter '*.whl').FullName
& $python -m ensurepip
& $python -m pip install --no-cache-dir --no-compile --no-index --no-deps @wheelPaths
& $python -m pip check
& $python -I -m uv --version
Fetch $lock.bridge "$work/bridge.zip"
Expand-Archive "$work/bridge.zip" "$work/bridge-source"
$bridge = (Get-ChildItem "$work/bridge-source" -Directory).FullName
Move-Item $bridge "$payload/bridge"
Copy-Item "$PSScriptRoot/runtime-lock.json" $payload
Copy-Item "$PSScriptRoot/SOLVER-NOTICES.txt" $payload
Copy-Item "$PSScriptRoot/SOLVER-NOTICES.txt" $out
Copy-Item "$PSScriptRoot/README.md" "$payload/INSTALL.md"
New-Item -ItemType Directory "$payload/licenses" | Out-Null
foreach ($file in ('LICENSE-MIT','LICENSE-APACHE','NOTICE','THIRD-PARTY-LICENSES.txt')) {
    Copy-Item "$source/$file" "$payload/licenses"
}
if ($IsWindows) {
    foreach ($exe in ('fieldcraft.exe','fieldcraft-cli.exe')) { Copy-Item "$build/$exe" $payload }
    Copy-Item "$PSScriptRoot/fieldcraft-ngsolve.cmd" $payload
    Fetch $lock.inno "$work/innosetup.exe"
    $compiler = "$work/inno"
    $install = Start-Process "$work/innosetup.exe" -ArgumentList "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER /DIR=`"$compiler`"" -Wait -PassThru
    if ($install.ExitCode) { throw "Inno compiler setup returned $($install.ExitCode)" }
    $arch = if ($Label -eq 'windows-arm64') { 'arm64 and x64compatible' } else { 'x64os' }
    & "$compiler/ISCC.exe" "/DAllowed=$arch" "/DStage=$payload" "/DLabel=$Label" "/O$out" "$PSScriptRoot/fieldcraft.iss"
} else {
    foreach ($exe in ('fieldcraft','fieldcraft-cli')) { Copy-Item "$build/$exe" $payload }
    chmod 755 "$payload/fieldcraft" "$payload/fieldcraft-cli"
    $deb = "$work/debian"
    New-Item -ItemType Directory -Force "$deb/opt","$deb/DEBIAN","$deb/usr/bin","$deb/usr/share/applications","$deb/usr/share/icons/hicolor/128x128/apps" | Out-Null
    Move-Item $payload "$deb/opt/fieldcraft-ngsolve"
    Copy-Item "$PSScriptRoot/control" "$deb/DEBIAN/control"
    Copy-Item "$PSScriptRoot/fieldcraft-ngsolve.sh" "$deb/usr/bin/fieldcraft-ngsolve"
    chmod 755 "$deb/usr/bin/fieldcraft-ngsolve"
    ln -s /opt/fieldcraft-ngsolve/fieldcraft "$deb/usr/bin/fieldcraft-ngsolve-gui"
    ln -s /opt/fieldcraft-ngsolve/fieldcraft-cli "$deb/usr/bin/fieldcraft-ngsolve-cli"
    Copy-Item "$PSScriptRoot/fieldcraft-ngsolve.desktop" "$deb/usr/share/applications"
    Copy-Item "$source/assets/app-icon/icon.iconset/icon_128x128.png" "$deb/usr/share/icons/hicolor/128x128/apps/fieldcraft-ngsolve.png"
    dpkg-deb --root-owner-group -Zxz -z3 --build $deb "$out/fieldcraft-ngsolve-linux-x64.deb"
}
Get-ChildItem $out -File | ForEach-Object {
    $hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $($_.Name)" | Set-Content -Encoding ascii ($_.FullName + '.sha256')
}
