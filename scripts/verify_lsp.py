#!/usr/bin/env python3
import json
import subprocess
import sys

LSP_BIN = "./target/release/rspp-lsp"

def send(proc, payload):
    raw = json.dumps(payload).encode("utf-8")
    header = f"Content-Length: {len(raw)}\r\n\r\n".encode("ascii")
    proc.stdin.write(header + raw)
    proc.stdin.flush()

def recv(proc):
    headers = {}
    while True:
        line = proc.stdout.readline().decode("ascii")
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        k, v = line.split(":", 1)
        headers[k.strip().lower()] = v.strip()
    length = int(headers["content-length"])
    body = proc.stdout.read(length).decode("utf-8")
    return json.loads(body)

def main():
    proc = subprocess.Popen(
        [LSP_BIN],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    send(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
    init_resp = recv(proc)
    assert init_resp["id"] == 1
    assert "completionProvider" in init_resp["result"]["capabilities"]

    uri = "file:///test_demo.rspp"
    bad_src = "fn main():\n    let buf = memory::alloc(64)\n    sdr::\n    power::set_voltage(\"vcore\", 2.1V)\n"
    send(proc, {
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {"textDocument": {"uri": uri, "languageId": "rustpp", "version": 1, "text": bad_src}},
    })
    diag_notif = recv(proc)
    assert diag_notif["method"] == "textDocument/publishDiagnostics"
    codes = [d["code"] for d in diag_notif["params"]["diagnostics"]]
    print("LSP Diagnostics:", codes)
    assert "LG001" in codes, f"Expected LG001 leak guard diag, got {codes}"
    assert "HW302" in codes, f"Expected HW302 voltage diag, got {codes}"

    send(proc, {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/completion",
        "params": {"textDocument": {"uri": uri}, "position": {"line": 2, "character": 9}},
    })
    comp_resp = recv(proc)
    labels = [it["label"] for it in comp_resp["result"]]
    print("SDR Completions sample:", labels[:6])
    assert "capture_iq" in labels and "fft_spectrum" in labels

    send(proc, {
        "jsonrpc": "2.0",
        "id": 3,
        "method": "textDocument/codeAction",
        "params": {"textDocument": {"uri": uri}, "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 10}}},
    })
    act_resp = recv(proc)
    titles = [a["title"] for a in act_resp["result"]]
    print("Leak Guard CodeActions:", titles)
    assert any("LG001" in t for t in titles)

    send(proc, {"jsonrpc": "2.0", "id": 4, "method": "shutdown", "params": {}})
    _ = recv(proc)
    send(proc, {"jsonrpc": "2.0", "method": "exit"})
    proc.wait(timeout=3)
    print("LSP VERIFICATION PASSED!")

if __name__ == "__main__":
    main()
