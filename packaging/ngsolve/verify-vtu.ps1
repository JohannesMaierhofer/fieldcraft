# Exercise the VTU reader and its portable project format through the installed CLI.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Cli,
    [Parameter(Mandatory)][string]$OutputDir
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$fixture = Join-Path $PSScriptRoot '../../docs/recipes/vtu-bending-strip'
$work = New-Item -ItemType Directory (Join-Path ([IO.Path]::GetTempPath()) ('fieldcraft-vtu-' + [guid]::NewGuid()))

function Invoke-Session([array]$Commands) {
    $id = 1
    $requests = @(
        @{jsonrpc='2.0'; id=$id; method='initialize'; params=@{protocolVersion='2025-06-18'; capabilities=@{}}}
        @{jsonrpc='2.0'; method='notifications/initialized'}
        foreach ($command in $Commands) {
            $id++
            @{jsonrpc='2.0'; id=$id; method='tools/call'; params=@{name='execute'; arguments=$command}}
        }
    )
    $lines = $requests | ForEach-Object { ConvertTo-Json $_ -Compress -Depth 8 }
    $replies = @($lines | & $Cli mcp --headless | ForEach-Object { ConvertFrom-Json $_ })
    if ($replies.Count -ne 1 + $Commands.Count) { throw 'Unexpected number of CLI replies' }
    foreach ($reply in $replies) {
        if ($reply.error -or $reply.result.isError) { throw (ConvertTo-Json $reply -Depth 8) }
        if ($reply.id -gt 1) { ConvertFrom-Json $reply.result.content[0].text }
    }
}

try {
    $source = New-Item -ItemType Directory (Join-Path $work 'grid')
    foreach ($file in 'bending.pvd','start.vtu','end.vtu') {
        Copy-Item (Join-Path $fixture $file) $source
    }
    $project = Join-Path $work 'embedded.fieldcraft'
    $import = @(Invoke-Session @(
        @{command='results.import.vtu'; params=@{path=(Join-Path $source 'start.vtu')}}
        @{command='results.import.vtu'; params=@{path=(Join-Path $source 'bending.pvd')}}
        @{command='results.external.view'; params=@{frame=1; association='point'; field='displacement'; displacement='displacement'; scale=8}}
        @{command='file.save_as'; params=@{path=$project}}
    ))
    if ($import[0].frames -ne 1 -or $import[1].frames -ne 2 -or $import[1].time -ne 0) {
        throw 'The installed VTU reader did not import the expected grid and time series'
    }
    Remove-Item -LiteralPath $source -Recurse -Force
    $reopened = @(Invoke-Session @(
        @{command='file.open'; params=@{path=$project}}
        @{command='results.external.view'; params=@{}}
    ))
    foreach ($frame in $import[2],$reopened[1]) {
        if ($frame.frames -ne 2 -or $frame.frame -ne 1 -or $frame.time -ne 1 -or
            $frame.points -ne 36 -or $frame.cells -ne 8 -or
            $frame.point_fields[0].name -ne 'displacement' -or $frame.point_fields[0].components -ne 3 -or
            $frame.view.displacement -ne 'displacement' -or $frame.view.scale -ne 8) {
            throw ('Unexpected bending-strip results: ' + (ConvertTo-Json $frame -Depth 8))
        }
    }
    New-Item -ItemType Directory -Force $OutputDir | Out-Null
    @{imported=$import[2]; reopened_without_sources=$reopened[1]} |
        ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 (Join-Path $OutputDir 'vtu-import.json')
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force
}
