# Reuse the exact application binaries tested by the initial installer workflow.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Label, [Parameter(Mandatory)][string]$Target)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$repository = 'JohannesMaierhofer/fieldcraft'
$run = '45048055587'
$head = 'd2134c4f14ac98d2b317766fa2498efda831b125'
try {
    $trusted = gh api "repos/$repository/actions/runs/$run" | ConvertFrom-Json
} catch {
    # The initial installer run is only an optional seed for the binary cache.
    Write-Warning "Previous run unavailable. Building the pinned FieldCraft source."
    "restored=false" >> $env:GITHUB_OUTPUT
    return
}
if ($trusted.name -ne 'External solver installers' -or $trusted.head_sha -ne $head -or $trusted.head_repository.full_name -ne $repository) {
    throw 'Previous installer run provenance differs from the selected build.'
}
$wanted = "fieldcraft-ngsolve-$Label"
$found = $null
for ($attempt = 0; $attempt -lt 80; $attempt++) {
    $found = (gh api "repos/$repository/actions/runs/$run/artifacts" | ConvertFrom-Json).artifacts |
        Where-Object { $_.name -eq $wanted -and -not $_.expired }
    if ($found) { break }
    $job = (gh api "repos/$repository/actions/runs/$run/jobs" | ConvertFrom-Json).jobs |
        Where-Object { $_.name.EndsWith(", $Label)") }
    if (-not $job -or $job.status -eq 'completed') { break }
    Write-Output "Waiting for the current $Label native build before reusing its binaries."
    Start-Sleep -Seconds 45
}
if (-not $found) {
    "restored=false" >> $env:GITHUB_OUTPUT
    return
}
$work = Join-Path ([IO.Path]::GetTempPath()) ('fieldcraft-core-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $work | Out-Null
gh run download $run --repo $repository --name $wanted --dir $work
$extension = if ($IsWindows) { '.exe' } else { '.deb' }
$setup = @(Get-ChildItem -Recurse $work -File | Where-Object Extension -eq $extension)
if ($setup.Count -ne 1) { throw 'Expected one previous installer' }
$file = $setup[0].FullName
$hash = (Get-Content -Raw ($file + '.sha256')).Split(' ')[0]
if ($hash -ne (Get-FileHash $file -Algorithm SHA256).Hash) { throw 'Installer checksum mismatch' }
$core = "$work/installed"
if ($IsWindows) {
    $install = Start-Process $file -ArgumentList "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /DIR=`"$core`"" -Wait -PassThru
    if ($install.ExitCode) { throw "Previous installer returned $($install.ExitCode)" }
    $suffix = '.exe'
} else {
    dpkg-deb --extract $file "$work/unpacked"
    $core = "$work/unpacked/opt/fieldcraft-ngsolve"
    $suffix = ''
}
$destination = "app-source/target/$Target/release"
New-Item -ItemType Directory -Force $destination | Out-Null
foreach ($exe in ('fieldcraft','fieldcraft-cli')) { Copy-Item "$core/$exe$suffix" $destination }
Copy-Item "$core/licenses/THIRD-PARTY-LICENSES.txt" 'app-source'
if ($IsWindows) {
    $remove = Start-Process "$core/unins000.exe" -ArgumentList "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART" -Wait -PassThru
    if ($remove.ExitCode) { throw 'Temporary extraction cleanup failed' }
}
Remove-Item -LiteralPath $work -Recurse -Force
"restored=true" >> $env:GITHUB_OUTPUT
