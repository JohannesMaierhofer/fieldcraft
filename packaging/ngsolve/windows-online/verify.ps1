[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][ValidateSet('x64','arm64')][string]$Arch,
    [Parameter(Mandatory)][string]$Work
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Run-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program returned $LASTEXITCODE" }
}

function Pe-Machine([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($reader.ReadUInt16() -ne 0x5a4d) { throw "Missing executable signature in $Path" }
        $stream.Position = 0x3c
        $stream.Position = $reader.ReadUInt32()
        if ($reader.ReadUInt32() -ne 0x4550) { throw "Missing PE signature in $Path" }
        return ('0x{0:x4}' -f $reader.ReadUInt16())
    } finally { $reader.Dispose() }
}

if (Test-Path -LiteralPath $Work) { throw "Use a fresh verification directory: $Work" }
[IO.Directory]::CreateDirectory($Work) | Out-Null
$Work = (Resolve-Path -LiteralPath $Work).Path
$Installer = (Resolve-Path -LiteralPath $Installer).Path
$prefix = Join-Path $Work 'program files'
$previousRecovery = $env:FIELDCRAFT_RECOVERY_DIR
$previousData = $env:LOCALAPPDATA
$previousCi = $env:CI
try {
    $env:FIELDCRAFT_RECOVERY_DIR = $null
    $env:CI = $null
    $env:LOCALAPPDATA = Join-Path $Work 'isolated local appdata'
    $prefs = Join-Path (Join-Path $env:LOCALAPPDATA 'FieldCraft') 'prefs.json'
    foreach ($attempt in 1,2,3) {
        # NSIS requires /D last, without quotes, including when the directory contains spaces.
        $process = Start-Process -FilePath $Installer -ArgumentList "/S /NOSHORTCUTS /D=$prefix" -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "Installer attempt $attempt returned $($process.ExitCode)" }
        if ($attempt -eq 1) {
            $generations = @(Get-ChildItem -LiteralPath $prefix -Directory -Filter 'g-*')
            if ($generations.Count -ne 1) { throw 'Expected exactly one complete generation' }
            $app = $generations[0].FullName
            $receipt = Get-Content -LiteralPath "$app\identity.json" -Raw | ConvertFrom-Json
            if ($generations[0].Name -cne ('g-' + $receipt.generation) -or $receipt.arch -ne $Arch -or
                (Get-FileHash -LiteralPath "$app\runtime-lock.json").Hash -ne $receipt.runtime_lock_sha256) {
                throw 'Runtime lock or generation differs from the receipt'
            }
            $fresh = Get-Content -LiteralPath $prefs -Raw | ConvertFrom-Json
            if ($fresh.show_donate_button -isnot [bool] -or $fresh.show_donate_button -or
                $fresh.update_check -isnot [bool] -or $fresh.update_check) { throw 'Incorrect fresh-profile switches' }
            [IO.File]::WriteAllText($prefs, '{"show_donate_button":true,"update_check":true,"reverse_zoom":true}')
            $customHash = (Get-FileHash -LiteralPath $prefs).Hash
            # Exercise actual bridge damage. Reinstall must replace the broken case dispatcher.
            Move-Item -LiteralPath "$app\bridge\case_run.py" -Destination (Join-Path $Work 'original-case_run.py')
        } elseif ((Get-FileHash -LiteralPath $prefs).Hash -ne $customHash) {
            throw 'Installer changed an existing preference file.'
        }
    }
    $preserved = @(Get-ChildItem -LiteralPath $prefix -Filter '.fieldcraft-preserved-*' -Directory -Force)
    if ($preserved.Count -ne 1 -or (Test-Path -LiteralPath (Join-Path $preserved[0].FullName 'bridge\case_run.py'))) {
        throw 'Missing or invalid preserved copy of the damaged generation'
    }
    if ((Get-FileHash -LiteralPath "$app\bridge\case_run.py").Hash -ne
        (Get-FileHash -LiteralPath (Join-Path $Work 'original-case_run.py')).Hash) {
        throw 'Missing case dispatcher was not repaired byte for byte'
    }
    $identity = Get-Content -LiteralPath "$app\identity.json" -Raw | ConvertFrom-Json
    if ($identity.desktop -ne '8b495f31389512c7f599ef17b5ee3a4d0cc39bc9' -or $identity.installer_revision -ne 'r2' -or
            $identity.bridge -ne '74393901cf1b6fe70e5956003f948c9fe5eee889') { throw 'Unexpected revision in the installed receipt.' }
    $expected = @{x64='0x8664'; arm64='0xaa64'}[$Arch]
    $gui = Pe-Machine "$app\fieldcraft.exe"
    $cli = Pe-Machine "$app\fieldcraft-cli.exe"
    $solver = Pe-Machine "$app\python\python.exe"
    if (($expected -ne $gui) -or ($expected -ne $cli) -or ($solver -ne '0x8664')) {
        throw "Unexpected PE architectures: GUI=$gui CLI=$cli solver=$solver"
    }
    Push-Location $Work
    try {
        Copy-Item "$app\bridge\samples\*" $Work
        Run-Checked "$app\fieldcraft-cli.exe" @('run','slab.json')
        Run-Checked "$app\fieldcraft-ngsolve.cmd" @('case','slab.case.json','computed')
        $result = Get-Content -LiteralPath 'computed\results.json' -Raw | ConvertFrom-Json
        $energy = @($result.integrals | Where-Object name -eq stored_energy)
        if ($energy.Count -ne 1 -or $energy[0].unit -ne 'J' -or
            [Math]::Abs(($energy[0].value / 4.4270939064e-14) - 1.0) -gt 1e-8 -or
            $result.fields.Count -ne 2) { throw 'Slab reference or field count differs.' }
        foreach ($name in 'view-1.json','view-2.json') {
            Run-Checked "$app\fieldcraft-cli.exe" @('run',(Join-Path 'computed' $name))
        }
        $utf8 = [Text.UTF8Encoding]::new($false)
        $potential = Get-Content -Raw -LiteralPath 'computed\field-1.json' | ConvertFrom-Json
        if ($potential.name -ne 'potential' -or $potential.components -ne 1) { throw 'Missing public slab potential' }
        # Exact synthetic deformation-check field u_y = 0.05*x on the unit-length slab;
        # the solved public capacitor potential is 1-x.
        $values = foreach ($v in $potential.values) { 0.0; 0.05 * (1.0 - $v); 0.0 }
        $field = [ordered]@{format='fieldcraft-nodal-field/1';mesh_sha256=$result.mesh_sha256;name='Affine shear';unit='m';
            components=3;node_ids=$potential.node_ids;values=$values}
        [IO.File]::WriteAllText((Join-Path $Work 'computed\shear.json'),($field | ConvertTo-Json -Depth 4 -Compress),$utf8)
        $result.fields += [pscustomobject]@{name='Affine shear';unit='m';file='shear.json'}
        [IO.File]::WriteAllText((Join-Path $Work 'computed\check-catalog.json'),($result | ConvertTo-Json -Depth 16),$utf8)
        $commands = @(
            @{command='file.open';params=@{path=(Join-Path $Work 'slab.fieldcraft')}},
            @{command='results.external.integrals';params=@{path=(Join-Path $Work 'computed\check-catalog.json');catalog_only=$true}},
            @{command='results.external.contour';params=@{entry='Affine shear';component='y'}},
            @{command='results.external.deformed';params=@{scale=2.5}},
            @{command='results.external.deformed';params=@{scale=0.25}},
            @{command='results.external.deformed';params=@{scale=0}}
        )
        [IO.File]::WriteAllText((Join-Path $Work 'shear-view.json'),(ConvertTo-Json -InputObject $commands -Depth 5),$utf8)
        Run-Checked "$app\fieldcraft-cli.exe" @('run','shear-view.json')
        $proof = [ordered]@{
            archive_sha256 = (Get-FileHash -LiteralPath $Installer).Hash.ToLowerInvariant()
            desktop = $identity.desktop
            bridge = $identity.bridge
            runtime_lock_sha256 = $identity.runtime_lock_sha256
            generation = $identity.generation
            damaged_case_repaired = $true
            install_count = 3
            fresh_profile_donate = $fresh.show_donate_button
            fresh_profile_update_check = $fresh.update_check
            existing_profile_byte_identical = $true
            windows_build = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuildNumber
            gui_machine = $gui
            cli_machine = $cli
            solver_machine = $solver
            runtime = $result.runtime
            fields = @($result.fields | Where-Object name -ne 'Affine shear')
            affine_shear_cli_scales = @(2.5,0.25,0.0)
            field_imports = 2
            electric_energy_J = $energy[0].value
        }
        $proof | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 verification.json
    } finally { Pop-Location }
} finally {
    $env:LOCALAPPDATA = $previousData
    $env:FIELDCRAFT_RECOVERY_DIR = $previousRecovery
    $env:CI = $previousCi
}
