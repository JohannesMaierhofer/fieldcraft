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

function Get-GenerationState([string]$Directory, [string]$ReceiptHash, [scriptblock]$Validate) {
    if (-not (Test-Path -LiteralPath $Directory)) { return 'new' }
    $entry = Get-Item -LiteralPath $Directory -Force
    if (-not $entry.PSIsContainer -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Installation path is occupied: $Directory"
    }
    $receipt = Join-Path $Directory 'identity.json'
    if ([IO.File]::Exists($receipt)) {
        # A mismatched receipt identifies another installation. Never move or remove it.
        Assert-Hash $receipt $ReceiptHash
        try { & $Validate $Directory | Out-Null } catch {
            Write-Host "Installed generation needs repair: $($_.Exception.Message)"
            return 'damaged'
        }
        return 'ready'
    }
    $pending = Join-Path $Directory '.fieldcraft-installing'
    if ([IO.File]::Exists($pending) -and [IO.File]::ReadAllText($pending) -ceq $ReceiptHash) {
        return 'interrupted'
    }
    throw "Unrecognized directory at the installation path: $Directory"
}

function Start-Generation([string]$Directory, [string]$ReceiptHash, [string]$State) {
    $root = [IO.Path]::GetDirectoryName($Directory)
    $id = [guid]::NewGuid().ToString('N')
    $claim = Join-Path $root ('.fieldcraft-claim-' + $id)
    $backup = $null
    [void][IO.Directory]::CreateDirectory($claim)
    try {
        [IO.File]::WriteAllText((Join-Path $claim '.fieldcraft-installing'), $ReceiptHash)
        if ($State -eq 'interrupted' -or $State -eq 'damaged') {
            # Recheck ownership after downloading, immediately before moving the old folder.
            $entry = Get-Item -LiteralPath $Directory -Force
            if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Reparse point at $Directory" }
            if ($State -eq 'damaged') { Assert-Hash (Join-Path $Directory 'identity.json') $ReceiptHash }
            elseif ([IO.File]::ReadAllText((Join-Path $Directory '.fieldcraft-installing')) -cne $ReceiptHash) {
                throw "Pending generation changed: $Directory"
            }
            $backup = Join-Path $root ('.fieldcraft-preserved-' + $id)
            [IO.Directory]::Move($Directory, $backup)
            Write-Host "Previous generation preserved for inspection at $backup"
        } elseif ($State -ne 'new') { throw "Unexpected generation state: $State" }
        try { [IO.Directory]::Move($claim, $Directory) } catch {
            if ($backup -and -not (Test-Path -LiteralPath $Directory)) { [IO.Directory]::Move($backup, $Directory) }
            throw
        }
    } finally {
        if ([IO.Directory]::Exists($claim)) {
            [IO.File]::Delete((Join-Path $claim '.fieldcraft-installing'))
            [IO.Directory]::Delete($claim)
        }
    }
}

function Check-Installation([string]$Directory, $Record) {
    Assert-Hash (Join-Path $Directory 'runtime-lock.json') $Record.runtime_lock_sha256
    foreach ($file in $Record.files.PSObject.Properties) {
        Assert-Hash (Join-Path $Directory $file.Name) $file.Value
    }
    Check-Runtime (Join-Path $Directory 'python\python.exe')
    if ($Record.installer_revision -eq 'r2') {
        Run-Checked (Join-Path $Directory 'fieldcraft-ngsolve.cmd') @('case','--help') | Out-Null
    } else {
        Run-Checked (Join-Path $Directory 'python\python.exe') @('-I','-m','uv','--version') | Out-Null
    }
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
$identity = Get-Content -LiteralPath "$PSScriptRoot\identity.json" -Raw | ConvertFrom-Json
Assert-Hash "$PSScriptRoot\runtime-lock.json" $identity.runtime_lock_sha256
if ($identity.installer_revision -notin 'r2','r4') { throw 'Unknown installer revision.' }
$legacyBridge = $identity.installer_revision -eq 'r2'
$generationPattern = if ($legacyBridge) { '^[a-z2-7]{52}$' } else { '^[0-9a-f]{32}$' }
if ($identity.generation -cnotmatch $generationPattern) { throw 'Invalid package generation identifier.' }
$lock = Get-Content -LiteralPath "$PSScriptRoot\runtime-lock.json" -Raw | ConvertFrom-Json
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
$version = 'g-' + $identity.generation
$root = [IO.Path]::GetFullPath($InstallRoot)
$application = Join-Path $root $version
$python = Join-Path $application 'python\python.exe'
$temp = Join-Path ([IO.Path]::GetTempPath()) ('fieldcraft-network-' + [guid]::NewGuid().ToString('N'))
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
    $receiptHash = (Get-FileHash -LiteralPath "$PSScriptRoot\identity.json" -Algorithm SHA256).Hash.ToLowerInvariant()
    $state = Get-GenerationState $application $receiptHash { param($path) Check-Installation $path $identity }
    if ($state -ne 'ready') {
        Download-Pinned $lock.windows.runtime (Join-Path $temp 'python.tar.gz')
        if ($legacyBridge) { Download-Pinned $lock.bridge (Join-Path $temp 'bridge.zip') }
        $wheels = @()
        foreach ($wheel in $lock.windows.wheels) {
            $path = Join-Path $temp $wheel.name
            Download-Pinned $wheel $path
            $wheels += $path
        }

        # Keep the Python prefix stable for entry-point scripts generated by pip.
        Start-Generation $application $receiptHash $state
        $system = "$env:SystemRoot\System32"
        if ($env:PROCESSOR_ARCHITEW6432) { $system = "$env:SystemRoot\Sysnative" }
        Run-Checked (Join-Path $system 'tar.exe') @('-xzf', (Join-Path $temp 'python.tar.gz'), '-C', $application)
        Run-Checked $python @('-I','-m','ensurepip')
        Run-Checked $python (@('-I','-m','pip','--isolated','--disable-pip-version-check','install','--no-index','--no-deps','--no-cache-dir','--no-compile') + $wheels)
        Run-Checked $python @('-I','-m','pip','--isolated','--disable-pip-version-check','check')
        if ($legacyBridge) {
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            $expanded = Join-Path $temp 'bridge'
            [IO.Compression.ZipFile]::ExtractToDirectory((Join-Path $temp 'bridge.zip'), $expanded)
            $bridge = Join-Path $expanded ("fieldcraft-ngsolve-" + $identity.bridge)
            if (-not (Test-Path -LiteralPath "$bridge\workflow.py" -PathType Leaf)) { throw 'Missing workflow.py in the pinned bridge.' }
            Copy-Item -LiteralPath $bridge -Destination (Join-Path $application 'bridge') -Recurse
        } else {
            Copy-Item -LiteralPath "$PSScriptRoot\examples" -Destination $application -Recurse
        }
        foreach ($name in 'fieldcraft.exe','fieldcraft-cli.exe') {
            Assert-Hash (Join-Path $PSScriptRoot $name) $identity.files.$name
            Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $application
        }
        foreach ($name in 'fieldcraft-ngsolve.cmd','runtime-lock.json','SOLVER-NOTICES.txt','INSTALL.txt') {
            Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $application
        }
        Copy-Item -LiteralPath "$PSScriptRoot\licenses" -Destination $application -Recurse
        Check-Installation $application $identity
        $pendingReceipt = Join-Path $application ('.ready-' + [guid]::NewGuid().ToString('N'))
        [IO.File]::Copy("$PSScriptRoot\identity.json", $pendingReceipt)
        [IO.File]::Move($pendingReceipt, (Join-Path $application 'identity.json'))
        [IO.File]::Delete((Join-Path $application '.fieldcraft-installing'))
    }

    Initialize-Preferences
    if (-not $SkipShortcuts) {
        $folder = Join-Path ([Environment]::GetFolderPath('Programs')) 'FieldCraft with NGSolve (network)'
        [IO.Directory]::CreateDirectory($folder) | Out-Null
        $shell = New-Object -ComObject WScript.Shell
        $examples = if ($legacyBridge) { 'bridge\samples' } else { 'examples' }
        foreach ($item in @(@('FieldCraft','fieldcraft.exe'),@('Solver examples',$examples))) {
            $link = $shell.CreateShortcut((Join-Path $folder ($item[0] + '.lnk')))
            $link.TargetPath = Join-Path $application $item[1]
            $link.WorkingDirectory = $application
            $link.Save()
        }
    }
    Write-Host "Installed $application"
} catch {
    Write-Host $_.Exception.Message
    throw
} finally {
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue }
    if ($owned) { $mutex.ReleaseMutex() }
    if ($null -ne $mutex) { $mutex.Dispose() }
    Stop-Transcript | Out-Null
}
