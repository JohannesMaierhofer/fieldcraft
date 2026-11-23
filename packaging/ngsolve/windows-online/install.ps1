[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InstallRoot,
    [switch]$SkipShortcuts
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-Hash([string]$Path, [string]$Expected) {
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $Expected) {
        throw "SHA-256 mismatch: $Path"
    }
}

function Download-Pinned($Record, [string]$Path) {
    if ($Record.url -notmatch '^https://(github\.com|codeload\.github\.com|files\.pythonhosted\.org)/') {
        throw "Unexpected download origin: $($Record.url)"
    }
    Write-Host "Downloading $($Record.url)"
    Invoke-WebRequest -UseBasicParsing -Uri $Record.url -OutFile $Path
    Assert-Hash $Path $Record.sha256
}

function Run-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program returned $LASTEXITCODE" }
}

function Check-Runtime([string]$Python) {
    Run-Checked $Python @('-I','-c', "import sys, sysconfig, numpy, ngsolve, netgen; assert sys.version_info[:3] == (3,13,18) and sysconfig.get_platform() == 'win-amd64' and numpy.__version__ == '2.3.5' and ngsolve.__version__ == netgen.__version__ == '6.2.2608'")
}

function Initialize-Preferences {
    # Match fieldcraft_io::recovery::default_dir() and the desktop's sibling prefs.json.
    if ($env:FIELDCRAFT_RECOVERY_DIR) {
        $parent = [IO.DirectoryInfo]::new($env:FIELDCRAFT_RECOVERY_DIR).Parent
        if ($null -eq $parent) { return }
        $directory = $parent.FullName
    } elseif ($env:CI) {
        $directory = Join-Path ([IO.Path]::GetTempPath()) 'fieldcraft-automation'
    } else {
        $directory = Join-Path $env:LOCALAPPDATA 'FieldCraft'
    }
    $prefs = Join-Path $directory 'prefs.json'
    if ([IO.File]::Exists($prefs)) { return }
    [void][IO.Directory]::CreateDirectory($directory)
    $pending = Join-Path $directory ('.prefs-' + [guid]::NewGuid().ToString('N') + '.tmp')
    try {
        [IO.File]::WriteAllText($pending, '{"show_donate_button":false,"update_check":false}', [Text.UTF8Encoding]::new($false))
        # File.Move in .NET Framework never replaces a destination installed by another writer.
        try { [IO.File]::Move($pending, $prefs) }
        catch [IO.IOException] { if (-not [IO.File]::Exists($prefs)) { throw } }
    } finally {
        [IO.File]::Delete($pending)
    }
}

# Windows PowerShell 5.1 uses the caller's TLS defaults on older installations.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'
$lock = Get-Content -LiteralPath "$PSScriptRoot\runtime-lock.json" -Raw | ConvertFrom-Json
$identity = Get-Content -LiteralPath "$PSScriptRoot\identity.json" -Raw | ConvertFrom-Json
$native = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment').PROCESSOR_ARCHITECTURE
$required = @{x64='AMD64'; arm64='ARM64'}[$identity.arch]
if ($native -ne $required) { throw "This installer requires $required Windows. Detected $native." }
$build = [int](Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuildNumber
if ($build -lt 17763) { throw 'Windows 10 version 1809 or later is required for the system tar.exe.' }
if ($required -eq 'ARM64' -and $build -lt 22000) { throw 'The solver on ARM64 requires Windows 11 x64 emulation.' }

$env:PYTHONHOME = $null
$env:PYTHONPATH = $null
$env:PYTHONNOUSERSITE = '1'
$env:PIP_CONFIG_FILE = 'NUL'
$env:PIP_DISABLE_PIP_VERSION_CHECK = '1'
$version = "$($identity.desktop.Substring(0,8))-$($identity.bridge.Substring(0,8))-$($identity.arch)"
$root = [IO.Path]::GetFullPath($InstallRoot)
$application = Join-Path $root $version
$python = Join-Path $application 'python\python.exe'
$temp = Join-Path ([IO.Path]::GetTempPath()) ('fieldcraft-network-' + [guid]::NewGuid().ToString('N'))
$created = $false
$mutex = $null
$owned = $false
$log = Join-Path ([IO.Path]::GetTempPath()) ("FieldCraft-NGSolve-network-" + [guid]::NewGuid().ToString('N') + ".log")
Start-Transcript -Path $log | Out-Null
try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    $mutex = [Threading.Mutex]::new($false, "Local\FieldCraft-NGSolve-Network-$version")
    try { $owned = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $owned = $true }
    if (-not $owned) { throw 'Another FieldCraft network installation is active.' }
    [IO.Directory]::CreateDirectory($temp) | Out-Null
    $receipt = Join-Path $application 'identity.json'
    if ([IO.Directory]::Exists($application)) {
        if (-not (Test-Path -LiteralPath $receipt -PathType Leaf)) {
            throw "Installation path already exists without a receipt. Select another destination: $application"
        }
        Assert-Hash $receipt (Get-FileHash -LiteralPath "$PSScriptRoot\identity.json").Hash
        foreach ($name in 'fieldcraft.exe','fieldcraft-cli.exe') { Assert-Hash (Join-Path $application $name) $identity.files.$name }
        if (-not (Test-Path -LiteralPath "$application\bridge\workflow.py" -PathType Leaf)) { throw "Incomplete bridge in $application" }
        Check-Runtime $python
    } else {
        Download-Pinned $lock.windows.runtime (Join-Path $temp 'python.tar.gz')
        Download-Pinned $lock.bridge (Join-Path $temp 'bridge.zip')
        $wheels = @()
        foreach ($wheel in $lock.windows.wheels) {
            $path = Join-Path $temp $wheel.name
            Download-Pinned $wheel $path
            $wheels += $path
        }

        # Keep the Python prefix stable for entry-point scripts generated by pip.
        [IO.Directory]::CreateDirectory($application) | Out-Null
        $created = $true
        $system = "$env:SystemRoot\System32"
        if ($env:PROCESSOR_ARCHITEW6432) { $system = "$env:SystemRoot\Sysnative" }
        Run-Checked (Join-Path $system 'tar.exe') @('-xzf', (Join-Path $temp 'python.tar.gz'), '-C', $application)
        Run-Checked $python @('-I','-m','ensurepip')
        Run-Checked $python (@('-I','-m','pip','--isolated','install','--no-index','--no-deps','--no-cache-dir','--no-compile') + $wheels)
        Run-Checked $python @('-I','-m','pip','--isolated','check')
        Check-Runtime $python

        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $archive = Join-Path $temp 'bridge.zip'
        $expanded = Join-Path $temp 'bridge'
        [IO.Compression.ZipFile]::ExtractToDirectory($archive, $expanded)
        $bridge = Join-Path $expanded ("fieldcraft-ngsolve-" + $identity.bridge)
        if (-not (Test-Path -LiteralPath "$bridge\workflow.py" -PathType Leaf)) { throw 'Missing workflow.py in the pinned bridge.' }
        Copy-Item -LiteralPath $bridge -Destination (Join-Path $application 'bridge') -Recurse
        foreach ($name in 'fieldcraft.exe','fieldcraft-cli.exe') {
            Assert-Hash (Join-Path $PSScriptRoot $name) $identity.files.$name
            Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $application
        }
        foreach ($name in 'fieldcraft-ngsolve.cmd','runtime-lock.json','SOLVER-NOTICES.txt','INSTALL.txt') {
            Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $application
        }
        Copy-Item -LiteralPath "$PSScriptRoot\licenses" -Destination $application -Recurse
        Copy-Item -LiteralPath "$PSScriptRoot\identity.json" -Destination $receipt
        $created = $false
    }

    Initialize-Preferences
    if (-not $SkipShortcuts) {
        $folder = Join-Path ([Environment]::GetFolderPath('Programs')) 'FieldCraft with NGSolve (network)'
        [IO.Directory]::CreateDirectory($folder) | Out-Null
        $shell = New-Object -ComObject WScript.Shell
        foreach ($item in @(@('FieldCraft','fieldcraft.exe'),@('Solver examples','bridge\samples'))) {
            $link = $shell.CreateShortcut((Join-Path $folder ($item[0] + '.lnk')))
            $link.TargetPath = Join-Path $application $item[1]
            $link.WorkingDirectory = $application
            $link.Save()
        }
    }
    Write-Host "Installed $application"
} catch {
    if ($created) { Remove-Item -LiteralPath $application -Recurse -Force -ErrorAction SilentlyContinue }
    Write-Host $_.Exception.Message
    throw
} finally {
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue }
    if ($owned) { $mutex.ReleaseMutex() }
    if ($null -ne $mutex) { $mutex.Dispose() }
    Stop-Transcript | Out-Null
}
