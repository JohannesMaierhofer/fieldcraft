[CmdletBinding()]
param(
    [string]$InstallerScript,
    [string]$InterruptTestRoot
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (!$InstallerScript) { $InstallerScript = Join-Path $PSScriptRoot 'install.ps1' }

# Load just the production state functions. Installation and downloading never run in this test.
$tokens = $null
$errors = $null
$tree = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $InstallerScript).Path,[ref]$tokens,[ref]$errors)
if ($errors) { throw $errors }
foreach ($name in 'Assert-Hash','Get-GenerationState','Start-Generation') {
    $fn = $tree.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($fn.Extent.Text))
}
if ($InterruptTestRoot) {
    if ([IO.Path]::GetFileName($InterruptTestRoot) -notmatch '^fieldcraft-recovery-test-[0-9a-f]{32}$') { throw 'Expected test-owned scratch directory' }
    $directory = Join-Path $InterruptTestRoot 'generation'
    $hash = (Get-FileHash -LiteralPath (Join-Path $InterruptTestRoot 'source-identity')).Hash.ToLowerInvariant()
    Start-Generation $directory $hash 'new'
    [IO.File]::WriteAllText((Join-Path $directory 'interrupted.txt'),'survives')
    # Terminate this separate writer process before it can publish a ready receipt.
    [Environment]::Exit(86)
}
$root = Join-Path ([IO.Path]::GetTempPath()) ('fieldcraft-recovery-test-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
try {
    $target = Join-Path $root 'generation'
    $source = Join-Path $root 'source-identity'
    [IO.File]::WriteAllText($source, 'test-only identity')
    $hash = (Get-FileHash -LiteralPath $source).Hash.ToLowerInvariant()
    $validate = { param($p) if (-not [IO.File]::Exists((Join-Path $p 'case_run.py'))) { throw 'Missing case module' } }
    if ((Get-GenerationState $target $hash $validate) -ne 'new') { throw 'Unexpected initial state' }
    $shell = [Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
    & $shell -NoProfile -NonInteractive -File $PSCommandPath -InstallerScript $InstallerScript -InterruptTestRoot $root
    if ($LASTEXITCODE -ne 86) { throw "Interrupted writer returned $LASTEXITCODE" }
    if ((Get-GenerationState $target $hash $validate) -ne 'interrupted') { throw 'Lost incomplete generation' }
    Start-Generation $target $hash 'interrupted'
    $saved = @(Get-ChildItem -LiteralPath $root -Filter '.fieldcraft-preserved-*' -Directory -Force)
    if ($saved.Count -ne 1 -or [IO.File]::ReadAllText((Join-Path $saved[0].FullName 'interrupted.txt')) -ne 'survives') {
        throw 'Interrupted data was not preserved'
    }
    Copy-Item -LiteralPath $source -Destination (Join-Path $target 'identity.json')
    if ((Get-GenerationState $target $hash $validate) -ne 'damaged') { throw 'Accepted incomplete ready receipt' }
    [IO.File]::WriteAllText((Join-Path $target 'case_run.py'),'test fixture')
    if ((Get-GenerationState $target $hash $validate) -ne 'ready') { throw 'Rejected complete generation' }

    $foreign = Join-Path $root 'unrelated'
    [void][IO.Directory]::CreateDirectory($foreign)
    $keep = Join-Path $foreign 'own.txt'
    [IO.File]::WriteAllText($keep,'untouched')
    $refused = $false
    try { [void](Get-GenerationState $foreign $hash $validate) } catch { $refused = $true }
    if (-not $refused -or [IO.File]::ReadAllText($keep) -ne 'untouched') { throw 'Foreign directory changed' }
    $refused = $false
    try { Start-Generation $foreign $hash 'interrupted' } catch { $refused = $true }
    if (-not $refused -or [IO.File]::ReadAllText($keep) -ne 'untouched') { throw 'Unmarked directory was moved on retry' }
    [IO.File]::WriteAllText((Join-Path $foreign 'identity.json'),'different lock or package')
    $refused = $false
    try { [void](Get-GenerationState $foreign $hash $validate) } catch { $refused = $true }
    if (-not $refused) { throw 'Foreign generation receipt accepted' }
    Write-Output 'Passed: terminated writer recovered, contents preserved, missing case rejected, ready generation accepted, foreign directory and mismatched receipt refused.'
} finally { Remove-Item -LiteralPath $root -Recurse -Force }
exit 0
