[CmdletBinding()]
param([Parameter(Mandatory)][ValidateSet('x64','arm64')][string]$Arch)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = (Resolve-Path "$PSScriptRoot/../../..").Path
$sha = (git -C $root rev-parse HEAD).Trim()
if ($sha -cne $env:FIELDCRAFT_BUILD_SHA -or $env:FIELDCRAFT_VERSION -cne '0.8.1-ngsolve.r5') {
    throw 'The release version and desktop revision must match this checkout.'
}
$target = @{x64='x86_64-pc-windows-msvc'; arm64='aarch64-pc-windows-msvc'}[$Arch]
$bin = Join-Path $env:CARGO_TARGET_DIR "$target/release"
$out = Join-Path $root 'dist/ngsolve'
$work = Join-Path ([IO.Path]::GetTempPath()) ("fieldcraft-r5-" + [guid]::NewGuid().ToString('N'))
$stage = Join-Path $work 'payload'
New-Item -ItemType Directory -Force "$stage/licenses","$stage/examples",$out | Out-Null

# Official portable compiler and its published digest from the NSIS 3.11 release.
$zip = Join-Path $work 'nsis.zip'
curl.exe --fail --location --retry 3 --silent --show-error --output $zip 'https://downloads.sourceforge.net/project/nsis/NSIS%203/3.11/nsis-3.11.zip'
if ((Get-FileHash $zip).Hash -ne 'c7d27f780ddb6cffb4730138cd1591e841f4b7edb155856901cdf5f214394fa1') {
    throw 'Unexpected NSIS tool archive.'
}
Expand-Archive -LiteralPath $zip -DestinationPath $work
$nsis = Join-Path $work 'nsis-3.11'
Copy-Item "$nsis/COPYING" "$stage/licenses/NSIS.txt"
foreach ($name in 'fieldcraft.exe','fieldcraft-cli.exe') { Copy-Item "$bin/$name" $stage }
foreach ($name in 'LICENSE-MIT','LICENSE-APACHE','NOTICE','THIRD-PARTY-LICENSES.txt') { Copy-Item "$root/$name" "$stage/licenses" }
foreach ($name in 'runtime-lock.json','SOLVER-NOTICES.txt','fieldcraft-ngsolve.cmd') { Copy-Item "$PSScriptRoot/../$name" $stage }
Copy-Item "$PSScriptRoot/install.ps1" $stage
Copy-Item "$PSScriptRoot/INSTALL-workspace.txt" "$stage/INSTALL.txt"
& "$PSScriptRoot/../fixtures.ps1" -Directory "$work/fixtures" -Scenario electrostatic
foreach ($name in 'slab.json','slab.analysis.json','potential.py') { Copy-Item "$work/fixtures/$name" "$stage/examples" }

$files = [ordered]@{}
foreach ($file in (Get-ChildItem -File -Recurse $stage | Sort-Object FullName)) {
    $name = [IO.Path]::GetRelativePath($stage, $file.FullName).Replace('\','/')
    if ($name -ne 'install.ps1') { $files[$name] = (Get-FileHash $file.FullName).Hash.ToLowerInvariant() }
}
$receipt = [ordered]@{
    arch = $Arch
    desktop = $sha
    installer_revision = 'r5'
    desktop_version = $env:FIELDCRAFT_VERSION
    runtime_lock_sha256 = $files['runtime-lock.json']
    installer_script_sha256 = (Get-FileHash "$stage/install.ps1").Hash.ToLowerInvariant()
    files = $files
}
$receiptPath = Join-Path $stage 'identity.json'
$receipt | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8NoBOM $receiptPath
$receipt['generation'] = (Get-FileHash $receiptPath).Hash.Substring(0,32).ToLowerInvariant()
$receipt | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8NoBOM $receiptPath
$name = "FieldCraft-NGSolve-0.8.1-r5-windows-$Arch-network.exe"
& "$nsis/makensis.exe" /V2 /DREVISION=5 "/DARCH=$Arch" "/DPAYLOAD=$stage" "/DOUTPUT=$out/$name" "$PSScriptRoot/setup.nsi"
Copy-Item "$stage/SOLVER-NOTICES.txt" $out
$hash = (Get-FileHash "$out/$name").Hash.ToLowerInvariant()
"$hash  $name" | Set-Content -Encoding ascii "$out/$name.sha256"
