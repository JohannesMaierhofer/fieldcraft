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
app = subprocess.Popen(["target/debug/fieldcraft", "--control", str(port), str(cad)])
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
            bodies = call("engine.execute", command="geometry.bodies", params={})
            if len(bodies) == 2:
                break
        except OSError:
            pass
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
