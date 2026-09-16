#!/usr/bin/env python3
"""Check the proxy against a local bridge checkout with dummy credentials."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

fixture = Path(__file__).resolve().parent
proxy = fixture.parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("bridge_checkout", type=Path)
bridge = parser.parse_args().bridge_checkout.resolve()
parent = Path(os.environ.get("TMPDIR", "/tmp/agents"))
parent.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(prefix="acme-bridge-", dir=parent))
shutil.copytree(bridge / "src", work / "src")
shutil.copyfile(bridge / "Cargo.lock", work / "Cargo.lock")
manifest = (bridge / "Cargo.toml").read_text()
manifest += "\n[dev-dependencies]\nacme-proxy = { path = " + json.dumps(str(proxy)) + " }\n"
(work / "Cargo.toml").write_text(manifest)
shutil.copyfile(fixture / "proxy_tests.rs", work / "src/dns/proxy_tests.rs")
with (work / "src/dns/mod.rs").open("a") as output:
    output.write("\n#[cfg(test)]\nmod proxy_tests;\n")
subprocess.run(["cargo", "nextest", "run", "-E", "test(proxy_tests)", "--no-fail-fast"], cwd=work, check=True)
