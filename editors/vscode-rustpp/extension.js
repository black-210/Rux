const vscode = require("vscode");
const cp = require("child_process");

let lspProc = null;
let lspBuffer = Buffer.alloc(0);
let nextReqId = 1;
const pending = new Map();
let diagCollection = null;

function sendLsp(obj) {
  if (!lspProc || lspProc.killed) return;
  const json = JSON.stringify(obj);
  const frame = `Content-Length: ${Buffer.byteLength(json, "utf8")}\r\n\r\n${json}`;
  lspProc.stdin.write(frame);
}

function requestLsp(method, params) {
  return new Promise((resolve) => {
    const id = nextReqId++;
    pending.set(id, resolve);
    sendLsp({ jsonrpc: "2.0", id, method, params });
    setTimeout(() => {
      if (pending.has(id)) {
        pending.delete(id);
        resolve(null);
      }
    }, 2000);
  });
}

function handleLspData(chunk) {
  lspBuffer = Buffer.concat([lspBuffer, chunk]);
  while (true) {
    const sep = lspBuffer.indexOf("\r\n\r\n");
    if (sep === -1) break;
    const header = lspBuffer.slice(0, sep).toString("utf8");
    const m = /Content-Length:\s*(\d+)/i.exec(header);
    if (!m) {
      lspBuffer = lspBuffer.slice(sep + 4);
      continue;
    }
    const len = parseInt(m[1], 10);
    if (lspBuffer.length < sep + 4 + len) break;
    const body = lspBuffer.slice(sep + 4, sep + 4 + len).toString("utf8");
    lspBuffer = lspBuffer.slice(sep + 4 + len);
    try {
      const msg = JSON.parse(body);
      if (msg.id !== undefined && pending.has(msg.id)) {
        const res = pending.get(msg.id);
        pending.delete(msg.id);
        res(msg.result || null);
      } else if (msg.method === "textDocument/publishDiagnostics" && diagCollection && msg.params) {
        const uri = vscode.Uri.parse(msg.params.uri);
        const list = (msg.params.diagnostics || []).map((d) => {
          const r = new vscode.Range(d.range.start.line, d.range.start.character, d.range.end.line, d.range.end.character);
          const sev = d.severity === 2 ? vscode.DiagnosticSeverity.Warning : vscode.DiagnosticSeverity.Error;
          const diag = new vscode.Diagnostic(r, d.message, sev);
          diag.code = d.code;
          diag.source = "rust++";
          return diag;
        });
        diagCollection.set(uri, list);
      }
    } catch (_) {}
  }
}

function activate(context) {
  diagCollection = vscode.languages.createDiagnosticCollection("rustpp");
  context.subscriptions.push(diagCollection);

  const cfg = vscode.workspace.getConfiguration("rustpp");
  const lspBin = cfg.get("lspPath") || "rspp-lsp";
  const rsppBin = cfg.get("compilerPath") || "rspp";

  lspProc = cp.spawn(lspBin, [], { stdio: ["pipe", "pipe", "ignore"] });
  lspProc.stdout.on("data", handleLspData);
  requestLsp("initialize", { processId: process.pid, capabilities: {} });

  const syncDoc = (doc) => {
    if (doc && doc.languageId === "rustpp") {
      sendLsp({
        jsonrpc: "2.0",
        method: "textDocument/didOpen",
        params: { textDocument: { uri: doc.uri.toString(), languageId: "rustpp", version: doc.version, text: doc.getText() } }
      });
    }
  };

  vscode.workspace.textDocuments.forEach(syncDoc);
  context.subscriptions.push(vscode.workspace.onDidOpenTextDocument(syncDoc));
  context.subscriptions.push(vscode.workspace.onDidChangeTextDocument((e) => syncDoc(e.document)));

  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider(
      "rustpp",
      {
        async provideCompletionItems(doc, pos) {
          const res = await requestLsp("textDocument/completion", {
            textDocument: { uri: doc.uri.toString() },
            position: { line: pos.line, character: pos.character }
          });
          if (!Array.isArray(res)) return [];
          return res.map((it) => {
            const ci = new vscode.CompletionItem(it.label, vscode.CompletionItemKind.Function);
            ci.detail = it.detail;
            ci.documentation = it.documentation;
            return ci;
          });
        }
      },
      ":", "."
    )
  );

  context.subscriptions.push(
    vscode.languages.registerHoverProvider("rustpp", {
      async provideHover(doc, pos) {
        const res = await requestLsp("textDocument/hover", {
          textDocument: { uri: doc.uri.toString() },
          position: { line: pos.line, character: pos.character }
        });
        if (!res || !res.contents) return null;
        return new vscode.Hover(new vscode.MarkdownString(res.contents.value || ""));
      }
    })
  );

  context.subscriptions.push(
    vscode.languages.registerCodeActionsProvider("rustpp", {
      async provideCodeActions(doc, range) {
        const res = await requestLsp("textDocument/codeAction", {
          textDocument: { uri: doc.uri.toString() },
          range: { start: { line: range.start.line, character: 0 }, end: { line: range.end.line, character: 0 } }
        });
        if (!Array.isArray(res)) return [];
        return res.map((act) => {
          const ca = new vscode.CodeAction(act.title, vscode.CodeActionKind.QuickFix);
          const edit = new vscode.WorkspaceEdit();
          const changes = (act.edit && act.edit.changes) || {};
          for (const [u, edits] of Object.entries(changes)) {
            for (const ed of edits) {
              edit.replace(vscode.Uri.parse(u), new vscode.Range(ed.range.start.line, ed.range.start.character, ed.range.end.line, ed.range.end.character), ed.newText);
            }
          }
          ca.edit = edit;
          return ca;
        });
      }
    })
  );

  const runCmd = (subcmd) => {
    const ed = vscode.window.activeTextEditor;
    if (!ed) return;
    ed.document.save().then(() => {
      const term = vscode.window.createTerminal("rust++");
      term.show();
      term.sendText(`${rsppBin} ${subcmd} "${ed.document.fileName}"`);
    });
  };

  context.subscriptions.push(vscode.commands.registerCommand("rustpp.checkFile", () => runCmd("check")));
  context.subscriptions.push(vscode.commands.registerCommand("rustpp.fixFile", () => runCmd("fix")));
  context.subscriptions.push(vscode.commands.registerCommand("rustpp.runFile", () => runCmd("run")));
  context.subscriptions.push(vscode.commands.registerCommand("rustpp.buildFile", () => runCmd("build")));
}

function deactivate() {
  if (lspProc && !lspProc.killed) lspProc.kill();
}

module.exports = { activate, deactivate };
