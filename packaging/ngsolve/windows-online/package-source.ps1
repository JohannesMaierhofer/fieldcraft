[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('x64','arm64')][string]$Arch,
    [Parameter(Mandatory)][string]$SourceRoot,
    [Parameter(Mandatory)][string]$Output,
    [Parameter(Mandatory)][string]$MakeNsis,
    [Parameter(Mandatory)][string]$NsisLicense
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$revision = '8b495f31389512c7f599ef17b5ee3a4d0cc39bc9'
$bridge = '74393901cf1b6fe70e5956003f948c9fe5eee889'
$machine = @{x64=0x8664;arm64=0xaa64}[$Arch]
$target = @{x64='x86_64-pc-windows-msvc';arm64='aarch64-pc-windows-msvc'}[$Arch]

function Hash-File([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Check-PE([string]$Path, [int]$Code) {
    $stream = [IO.File]::OpenRead($Path)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($reader.ReadUInt16() -ne 0x5a4d) {throw "Missing DOS header in $Path"}
        $stream.Position = 0x3c
        $stream.Position = $reader.ReadUInt32()
        if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne $Code) {
            throw "Incorrect PE machine in $Path"
        }
    } finally { $reader.Dispose() }
}

function Generation-Id([string]$Receipt) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $bytes = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Receipt)) }
    finally { $sha.Dispose() }
    $alphabet = 'abcdefghijklmnopqrstuvwxyz234567'
    $bits = -join ($bytes | ForEach-Object { [Convert]::ToString($_,2).PadLeft(8,'0') })
    $bits = $bits.PadRight(260,'0')
    -join (0..51 | ForEach-Object { $alphabet[[Convert]::ToInt32($bits.Substring(5*$_,5),2)] })
}

$SourceRoot = (Resolve-Path -LiteralPath $SourceRoot).Path
$MakeNsis = (Resolve-Path -LiteralPath $MakeNsis).Path
$NsisLicense = (Resolve-Path -LiteralPath $NsisLicense).Path
$Output = [IO.Path]::GetFullPath($Output)
$name = "FieldCraft-NGSolve-0.8.1-r2-windows-$Arch-network.exe"
$installer = Join-Path $Output $name
if (Test-Path -LiteralPath $installer) {throw "Output already exists: $installer"}
$stage = Join-Path $Output "payload-$Arch"
if (Test-Path -LiteralPath $stage) {throw "Choose a fresh staging directory: $stage"}
$lock = Join-Path $SourceRoot 'packaging\ngsolve\runtime-lock.json'
$lockedBridge = (Get-Content -Raw -LiteralPath $lock | ConvertFrom-Json).bridge
if (([uri]$lockedBridge.url).Segments[-1] -ne $bridge) {throw 'Unexpected solver bridge revision'}
Push-Location $SourceRoot
try {
    $actual = git rev-parse HEAD
    if ($LASTEXITCODE -ne 0 -or $actual -ne $revision) {throw "Expected desktop source $revision"}
    $changes = @(git status --porcelain --untracked-files=normal)
    if ($LASTEXITCODE -ne 0 -or $changes.Count -ne 0) {throw 'Desktop source checkout must be clean'}
    $env:FIELDCRAFT_BUILD_SHA = $revision
    $env:FIELDCRAFT_VERSION = '0.8.1-ngsolve.r1'
    if (!$env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $SourceRoot 'target' }
    $env:CARGO_TARGET_DIR = [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
    if ($Arch -eq 'arm64') { $env:CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static' }
    & cargo build --release --locked --target $target -p fieldcraft -p fieldcraft-cli
    if ($LASTEXITCODE -ne 0) {throw "Native source build failed for $target"}
    & cargo run --release --locked -p xtask -- licences
    if ($LASTEXITCODE -ne 0) {throw 'Third-party license collection failed'}
    $bin = Join-Path $env:CARGO_TARGET_DIR "$target\release"
    $binaries = [ordered]@{}
    foreach ($file in 'fieldcraft.exe','fieldcraft-cli.exe') {
        $exe = Join-Path $bin $file
        Check-PE $exe $machine
        $binaries[$file] = Hash-File $exe
    }
    $native = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    if ($native -eq $Arch) {
        $cli = Join-Path $bin 'fieldcraft-cli.exe'
        $version = & $cli --version
        if ($LASTEXITCODE -ne 0 -or $version -ne 'fieldcraft-cli 0.8.1-ngsolve.r1') {throw "Unexpected version: $version"}
        $savedEncoding = [Console]::OutputEncoding
        try {
            [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
            $features = (& $cli features) -join "`n"
            $featureStatus = $LASTEXITCODE
        } finally { [Console]::OutputEncoding = $savedEncoding }
        $documented = (Get-Content -LiteralPath (Join-Path $SourceRoot 'docs\features.md') -Encoding UTF8) -join "`n"
        if ($featureStatus -ne 0 -or $features.TrimEnd() -cne $documented.TrimEnd() -or $features -notmatch 'results\.external\.deformed') {
            throw 'Installed target feature catalog differs from the desktop source'
        }
    }
    [void][IO.Directory]::CreateDirectory((Join-Path $stage 'licenses'))
    foreach ($file in $binaries.Keys) {Copy-Item -LiteralPath (Join-Path $bin $file) -Destination $stage}
    foreach ($file in 'LICENSE-MIT','LICENSE-APACHE','NOTICE','THIRD-PARTY-LICENSES.txt') {
        Copy-Item -LiteralPath (Join-Path $SourceRoot $file) -Destination (Join-Path $stage 'licenses')
    }
    Copy-Item -LiteralPath $NsisLicense -Destination (Join-Path $stage 'licenses\NSIS.txt')
    foreach ($file in 'SOLVER-NOTICES.txt','runtime-lock.json','fieldcraft-ngsolve.cmd') {
        Copy-Item -LiteralPath (Join-Path $SourceRoot "packaging\ngsolve\$file") -Destination $stage
    }
    foreach ($file in 'INSTALL.txt','install.ps1') {Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination $stage}
    $identity = [ordered]@{arch=$Arch;bridge=$bridge;desktop=$revision;files=$binaries;installer_revision='r2';runtime_lock_sha256=(Hash-File $lock)}
    $identity.generation = Generation-Id ($identity | ConvertTo-Json -Depth 4 -Compress)
    $utf8 = [Text.UTF8Encoding]::new($false)
    [IO.File]::WriteAllText((Join-Path $stage 'identity.json'), ($identity | ConvertTo-Json -Depth 4) + "`n", $utf8)
    & $MakeNsis /V2 "/DARCH=$Arch" "/DPAYLOAD=$stage" "/DOUTPUT=$installer" (Join-Path $PSScriptRoot 'setup.nsi')
    if ($LASTEXITCODE -ne 0) {throw "NSIS failed for $Arch"}
    $digest = Hash-File $installer
    [IO.File]::WriteAllText("$installer.sha256", "$digest  $name`n", $utf8)
    $files = [ordered]@{}
    Get-ChildItem -LiteralPath $stage -File -Recurse | Sort-Object FullName | ForEach-Object {
        $relative = $_.FullName.Substring($stage.Length+1).Replace('\','/')
        $files[$relative] = Hash-File $_.FullName
    }
    [ordered]@{installer=$name;sha256=$digest;bytes=(Get-Item $installer).Length;identity=$identity;
        target=$target;source=$revision;build_host=$native;native_feature_catalog_checked=($native -eq $Arch);payload=$files} |
        ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $Output "receipt-$Arch.json")
} finally { Pop-Location }
