"""Fork-only native GUI smoke test against a generated two-instance STEP assembly."""
import json
from pathlib import Path
import socket
import subprocess
import sys
import time

cad = Path(sys.argv[1]).resolve(strict=True)
out = Path("verification").resolve()
out.mkdir()
with socket.socket() as probe:
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]
app = subprocess.Popen(["target/debug/fieldcraft", "--control", str(port)])
def call(method, **params):
    with socket.create_connection(("127.0.0.1", port), timeout=30) as sock, sock.makefile("rwb") as channel:
        channel.write((json.dumps(dict(id=1, method=method, params=params))+"\n").encode())
        channel.flush()
        reply = json.loads(channel.readline())
        if not reply.get("ok"):
            raise RuntimeError(reply)
        return reply["result"]
try:
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        if app.poll() is not None:
            raise RuntimeError("GUI stopped during startup")
        try:
            call("ui.inspect")
            break
        except OSError:
            time.sleep(.3)
    else:
        raise RuntimeError("GUI control port did not open")
    call("ui.capture", path=str(out / "before.png"), width=1100, height=720, scale=1)
    call("ui.drop", paths=[str(cad)])
    while time.monotonic() < deadline:
        call("job.wait")
        try:
            bodies = call("engine.execute", command="geometry.bodies", params={})
        except RuntimeError as error:
            if "wait for it or cancel it" not in str(error):
                raise
            continue
        if len(bodies) == 2:
            break
        time.sleep(.3)
    else:
        raise RuntimeError("STEP bodies never appeared")
    call("ui.capture", path=str(out / "step.png"), width=1100, height=720, scale=1)
    (out / "bodies.json").write_text(json.dumps(bodies, indent=2))
    assert all(len(body["faces"]) == 6 for body in bodies), bodies
    print([(b["name"], len(b["faces"])) for b in bodies])
finally:
    app.terminate()
    try:
        app.wait(timeout=10)
    except subprocess.TimeoutExpired:
        app.kill()
        app.wait()
