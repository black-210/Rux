#!/usr/bin/env python3
import json
import subprocess
import sys

def send(proc, payload):
    body = json.dumps(payload).encode("utf-8")
    header = f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8")
    proc.stdin.write(header + body)
    proc.stdin.flush()

def recv(proc):
    headers = {}
    while True:
        line = proc.stdout.readline().decode("utf-8")
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        if ":" in line:
            k, v = line.split(":", 1)
            headers[k.strip().lower()] = v.strip()
    length = int(headers.get("content-length", "0"))
    if length <= 0:
        return None
    body = proc.stdout.read(length).decode("utf-8")
    return json.loads(body)

def main():
    lsp_bin = sys.argv[1] if len(sys.argv) > 1 else "./target/release/nabda-lsp"
    proc = subprocess.Popen(
        [lsp_bin],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    send(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}})
    init_resp = recv(proc)
    assert init_resp and "capabilities" in init_resp["result"], f"Bad init: {init_resp}"

    send(proc, {"jsonrpc": "2.0", "method": "initialized", "params": {}})

    # Open broken file and verify live publishDiagnostics
    broken_src = 'fn main()\n    let x = 1\n    x = 2\n    memory::'
    send(proc, {
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {"textDocument": {"uri": "file:///demo.nb", "languageId": "nabda", "version": 1, "text": broken_src}},
    })
    diag_msg = recv(proc)
    assert diag_msg["method"] == "textDocument/publishDiagnostics"
    diags = diag_msg["params"]["diagnostics"]
    assert len(diags) >= 2, f"Expected diagnostics, got {diags}"

    # Verify smart completion after `memory::`
    send(proc, {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/completion",
        "params": {"textDocument": {"uri": "file:///demo.nb"}, "position": {"line": 3, "character": 12}},
    })
    comp_resp = recv(proc)
    labels = [it["label"] for it in comp_resp["result"]["items"]]
    assert "alloc" in labels and "map_mmio" in labels and "zero_secure" in labels, f"Missing completions: {labels}"

    # Verify Quick Fix codeAction
    send(proc, {
        "jsonrpc": "2.0",
        "id": 3,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": {"uri": "file:///demo.nb"},
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 10}},
        },
    })
    ca_resp = recv(proc)
    assert len(ca_resp["result"]) >= 1, f"Expected codeActions, got {ca_resp}"

    send(proc, {"jsonrpc": "2.0", "id": 4, "method": "shutdown"})
    recv(proc)
    send(proc, {"jsonrpc": "2.0", "method": "exit"})
    proc.wait(timeout=3)
    print(f"LSP E2E PASSED: {len(diags)} live diagnostics, {len(labels)} completions, {len(ca_resp['result'])} Quick Fixes!")

if __name__ == "__main__":
    main()
