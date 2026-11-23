[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Python,
    [Parameter(Mandatory)][string]$Cli,
    [Parameter(Mandatory)][string]$Gui,
    [Parameter(Mandatory)][string]$Launcher,
    [Parameter(Mandatory)][string]$OutputDir
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$env:OPENBLAS_NUM_THREADS = '1'
$env:VECLIB_MAXIMUM_THREADS = '1'
$env:OMP_NUM_THREADS = '1'
$work = Join-Path $OutputDir 'magnetic-diffusion'
New-Item -ItemType Directory -Force $work | Out-Null
& "$PSScriptRoot/fixtures.ps1" -Directory $work -Scenario diffusion
Push-Location $work
try {
    & $Cli run mesh.json
    & $Launcher run slab.analysis.json decay
    & $Cli run decay/view-series-1.json
    Copy-Item decay/diffusion-check.json (Join-Path $OutputDir 'magnetic-diffusion-reference.json')
    $port = 28745
    $app = Start-Process $Gui -ArgumentList "--control $port" -PassThru -RedirectStandardError "$work/gui.stderr"
    try {
        $check = @'
import hashlib, json, math, sys, time
from pathlib import Path
from verify_ui import connect, open_run

out = Path(sys.argv[2]).resolve()
rpc = connect(int(sys.argv[1]))
manifest, record = open_run(rpc, 'decay')
series = json.loads((manifest.parent/record['series'][0]['file']).read_text())
times = [frame['time'] for frame in series['frames']]
assert len(times) == 17 and times[0] == 0 and math.isclose(times[-1], 0.00025464790891341067, rel_tol=1e-10)
rows = [json.loads((manifest.parent/f['file']).read_text())['values'][1::3] for f in series['frames']]
expected_range = [min(map(min,rows)),max(map(max,rows))]
rpc('engine.execute', dict(command='results.external.integrals', params=dict(path=str(manifest), catalog_only=True)))
series_path = str(manifest.parent / record['series'][0]['file'])
opened = rpc('engine.execute', dict(command='results.external.series', params=dict(path=series_path, component='y')))
assert opened['shown'] and opened['frames'] == len(times) and opened['time_unit'] == 's', opened
assert all(math.isclose(x,y,rel_tol=1e-6,abs_tol=1e-12) for x,y in zip(opened['range'],expected_range,strict=True)), opened
def playback(**params):
    return rpc('engine.execute',dict(command='results.external.playback',params=params))
def capture(tag):
    time.sleep(.3)
    names = {}
    for method, suffix, size in [('ui.capture','window',(1200,760)),('ui.render','model',(640,440))]:
        path = out/f'diffusion-{tag}-{suffix}.png'
        rpc(method, dict(path=str(path),width=size[0],height=size[1]))
        if path.stat().st_size < 1000:
            raise ValueError(f'Missing {method} output')
        names[suffix] = hashlib.sha256(path.read_bytes()).hexdigest()
    return names
rpc('ui.move', {'x':3,'y':3})
first = playback()
assert first['frame'] == 0 and not first['playing']
first_images = capture('first')
middle = playback(frame=8)
assert middle['frame'] == 8 and not middle['playing'] and middle['time'] == times[8], middle
assert playback(playing=True)['playing']
deadline = time.monotonic()+25
while (advanced := playback())['frame'] <= 8:
    if time.monotonic() > deadline:
        raise TimeoutError('Native playback did not advance')
    time.sleep(.1)
paused = playback(playing=False)
time.sleep(.3)
assert not paused['playing'] and playback()['frame'] == paused['frame']
if paused['frame'] == len(times)-1:
    playback(frame=8)
playback(playing=True)
deadline = time.monotonic()+25
while (last := playback())['frame'] != len(times)-1 or last['playing']:
    if time.monotonic() > deadline:
        raise TimeoutError('Native playback did not stop on the last frame')
    time.sleep(.1)
assert last['time'] == times[-1] and last['time_unit'] == 's'
last_images = capture('last')
assert first_images['model'] != last_images['model'], 'The rendered mesh did not change during playback'
restart = playback(playing=True)
assert restart['frame'] == 0 and restart['playing'], restart
playback(playing=False)
state = rpc('ui.inspect',{})
proof = dict(opened=opened, range_from_solver=expected_range, first=first, scrubbed=middle, advanced=advanced,
             paused=paused, last=last, restart=restart, first_images=first_images, last_images=last_images,
             renderer=state.get('renderer'), adapter=state.get('adapter'))
(out/'diffusion-playback.json').write_text(json.dumps(proof,indent=2),encoding='utf-8')
print(json.dumps(proof,ensure_ascii=True))
'@
        & $Python -c $check $port $OutputDir
    } finally {
        if (-not $app.HasExited) { $app.Kill(); $app.WaitForExit() }
        elseif ($app.ExitCode -ne 0) { Get-Content "$work/gui.stderr" -Tail 30 }
    }
} finally { Pop-Location }
