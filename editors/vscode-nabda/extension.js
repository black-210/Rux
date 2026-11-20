const vscode = require('vscode');
const cp = require('child_process');
const path = require('path');

let lspProcess = null;
let requestId = 1;
const pending = new Map();
let diagCollection = null;
let rxBuffer = Buffer.alloc(0);

function getConfig() {
  return vscode.workspace.getConfiguration('nabda');
}

function resolveBinary(settingKey, fallbackName) {
  const configured = getConfig().get(settingKey) || fallbackName;
  if (configured !== fallbackName) return configured;

  // Prefer workspace target/release or target/debug binary if open in workspace
  const folders = vscode.workspace.workspaceFolders;
  if (folders && folders.length > 0) {
    const root = folders[0].uri.fsPath;
    const candidates = [
      path.join(root, 'target', 'release', fallbackName),
      path.join(root, 'target', 'debug', fallbackName),
    ];
    for (const c of candidates) {
      if (require('fs').existsSync(c)) return c;
    }
  }
  return configured;
}

function sendLspMessage(payload) {
  if (!lspProcess || !lspProcess.stdin.writable) return;
  const json = JSON.stringify(payload);
  const bytes = Buffer.from(json, 'utf8');
  const header = Buffer.from(`Content-Length: ${bytes.length}\r\n\r\n`, 'utf8');
  lspProcess.stdin.write(Buffer.concat([header, bytes]));
}

function sendRequest(method, params) {
  const id = requestId++;
  return new Promise((resolve) => {
    pending.set(id, resolve);
    sendLspMessage({ jsonrpc: '2.0', id, method, params });
    setTimeout(() => {
      if (pending.has(id)) {
        pending.delete(id);
        resolve(null);
      }
    }, 3000);
  });
}

function sendNotification(method, params) {
  sendLspMessage({ jsonrpc: '2.0', method, params });
}

function handleIncomingBytes(chunk) {
  rxBuffer = Buffer.concat([rxBuffer, chunk]);
  while (true) {
    const sepIdx = rxBuffer.indexOf('\r\n\r\n');
    if (sepIdx === -1) break;
    const header = rxBuffer.slice(0, sepIdx).toString('utf8');
    const match = /Content-Length:\s*(\d+)/i.exec(header);
    if (!match) {
      rxBuffer = rxBuffer.slice(sepIdx + 4);
      continue;
    }
    const len = parseInt(match[1], 10);
    const totalLen = sepIdx + 4 + len;
    if (rxBuffer.length < totalLen) break;
    const body = rxBuffer.slice(sepIdx + 4, totalLen).toString('utf8');
    rxBuffer = rxBuffer.slice(totalLen);
    try {
      const msg = JSON.parse(body);
      if (msg.id !== undefined && pending.has(msg.id)) {
        const resolve = pending.get(msg.id);
        pending.delete(msg.id);
        resolve(msg.result);
      } else if (msg.method === 'textDocument/publishDiagnostics' && msg.params) {
        updateDiagnostics(msg.params.uri, msg.params.diagnostics || []);
      }
    } catch (_) {}
  }
}

function toVscodeRange(r) {
  return new vscode.Range(
    new vscode.Position(r.start.line, r.start.character),
    new vscode.Position(r.end.line, r.end.character)
  );
}

function updateDiagnostics(uriStr, lspDiags) {
  if (!diagCollection) return;
  const uri = vscode.Uri.parse(uriStr);
  const diags = lspDiags.map((d) => {
    const sev =
      d.severity === 1
        ? vscode.DiagnosticSeverity.Error
        : d.severity === 2
        ? vscode.DiagnosticSeverity.Warning
        : vscode.DiagnosticSeverity.Information;
    const vd = new vscode.Diagnostic(toVscodeRange(d.range), d.message, sev);
    vd.code = d.code;
    vd.source = d.source || 'nabda-compiler';
    return vd;
  });
  diagCollection.set(uri, diags);
}

function startLsp(context) {
  const lspBin = resolveBinary('lspPath', 'nabda-lsp');
  lspProcess = cp.spawn(lspBin, [], { stdio: ['pipe', 'pipe', 'pipe'] });
  lspProcess.stdout.on('data', handleIncomingBytes);
  lspProcess.on('error', (err) => {
    vscode.window.showWarningMessage(`Nabda LSP (${lspBin}) could not start: ${err.message}`);
  });

  sendRequest('initialize', {
    processId: process.pid,
    rootUri: vscode.workspace.workspaceFolders?.[0]?.uri.toString() || null,
    capabilities: {},
  }).then(() => {
    sendNotification('initialized', {});
    for (const doc of vscode.workspace.textDocuments) {
      if (doc.languageId === 'nabda') {
        sendNotification('textDocument/didOpen', {
          textDocument: {
            uri: doc.uri.toString(),
            languageId: 'nabda',
            version: doc.version,
            text: doc.getText(),
          },
        });
      }
    }
  });

  context.subscriptions.push({
    dispose: () => {
      if (lspProcess) {
        try {
          sendRequest('shutdown', {});
          sendNotification('exit', {});
          lspProcess.kill();
        } catch (_) {}
      }
    },
  });
}

function runInTerminal(cmd) {
  const term = vscode.window.activeTerminal || vscode.window.createTerminal('Nabda');
  term.show();
  term.sendText(cmd);
}

function activate(context) {
  diagCollection = vscode.languages.createDiagnosticCollection('nabda');
  context.subscriptions.push(diagCollection);

  startLsp(context);

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (doc.languageId === 'nabda') {
        sendNotification('textDocument/didOpen', {
          textDocument: { uri: doc.uri.toString(), languageId: 'nabda', version: doc.version, text: doc.getText() },
        });
      }
    }),
    vscode.workspace.onDidChangeTextDocument((ev) => {
      if (ev.document.languageId === 'nabda') {
        sendNotification('textDocument/didChange', {
          textDocument: { uri: ev.document.uri.toString(), version: ev.document.version },
          contentChanges: [{ text: ev.document.getText() }],
        });
      }
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.languageId === 'nabda' && getConfig().get('autoFixOnSave')) {
        const bin = resolveBinary('executablePath', 'nabda');
        cp.execFile(bin, ['fix', doc.uri.fsPath], () => {});
      }
    })
  );

  // Smart Autocomplete (triggered on ':' and '.')
  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider(
      'nabda',
      {
        async provideCompletionItems(doc, pos) {
          const res = await sendRequest('textDocument/completion', {
            textDocument: { uri: doc.uri.toString() },
            position: { line: pos.line, character: pos.character },
          });
          if (!res || !res.items) return [];
          return res.items.map((it) => {
            const kindMap = {
              3: vscode.CompletionItemKind.Function,
              6: vscode.CompletionItemKind.Variable,
              9: vscode.CompletionItemKind.Module,
              14: vscode.CompletionItemKind.Keyword,
              21: vscode.CompletionItemKind.Constant,
            };
            const ci = new vscode.CompletionItem(it.label, kindMap[it.kind] || vscode.CompletionItemKind.Text);
            ci.detail = it.detail;
            ci.documentation = it.documentation ? new vscode.MarkdownString(it.documentation) : undefined;
            if (it.insertText) {
              ci.insertText =
                it.insertTextFormat === 2 ? new vscode.SnippetString(it.insertText) : it.insertText;
            }
            return ci;
          });
        },
      },
      ':',
      '.'
    )
  );

  // Hover Documentation
  context.subscriptions.push(
    vscode.languages.registerHoverProvider('nabda', {
      async provideHover(doc, pos) {
        const res = await sendRequest('textDocument/hover', {
          textDocument: { uri: doc.uri.toString() },
          position: { line: pos.line, character: pos.character },
        });
        if (!res || !res.contents) return null;
        return new vscode.Hover(new vscode.MarkdownString(res.contents.value));
      },
    })
  );

  // Quick Fix Code Actions (Lightbulb auto-repair)
  context.subscriptions.push(
    vscode.languages.registerCodeActionsProvider('nabda', {
      async provideCodeActions(doc, range) {
        const res = await sendRequest('textDocument/codeAction', {
          textDocument: { uri: doc.uri.toString() },
          range: {
            start: { line: range.start.line, character: range.start.character },
            end: { line: range.end.line, character: range.end.character },
          },
        });
        if (!Array.isArray(res)) return [];
        return res.map((a) => {
          const action = new vscode.CodeAction(a.title, vscode.CodeActionKind.QuickFix);
          action.isPreferred = Boolean(a.isPreferred);
          if (a.edit && a.edit.changes) {
            const wsEdit = new vscode.WorkspaceEdit();
            for (const [u, edits] of Object.entries(a.edit.changes)) {
              const targetUri = vscode.Uri.parse(u);
              for (const e of edits) {
                wsEdit.replace(targetUri, toVscodeRange(e.range), e.newText);
              }
            }
            action.edit = wsEdit;
          }
          return action;
        });
      },
    })
  );

  // Signature Help
  context.subscriptions.push(
    vscode.languages.registerSignatureHelpProvider(
      'nabda',
      {
        async provideSignatureHelp(doc, pos) {
          const res = await sendRequest('textDocument/signatureHelp', {
            textDocument: { uri: doc.uri.toString() },
            position: { line: pos.line, character: pos.character },
          });
          if (!res || !res.signatures) return null;
          const sh = new vscode.SignatureHelp();
          sh.activeSignature = res.activeSignature || 0;
          sh.activeParameter = res.activeParameter || 0;
          sh.signatures = res.signatures.map((s) => {
            const info = new vscode.SignatureInformation(s.label, s.documentation);
            info.parameters = (s.parameters || []).map((p) => new vscode.ParameterInformation(p.label));
            return info;
          });
          return sh;
        },
      },
      '(',
      ','
    )
  );

  // Go to Definition
  context.subscriptions.push(
    vscode.languages.registerDefinitionProvider('nabda', {
      async provideDefinition(doc, pos) {
        const res = await sendRequest('textDocument/definition', {
          textDocument: { uri: doc.uri.toString() },
          position: { line: pos.line, character: pos.character },
        });
        if (!res || !res.uri) return null;
        return new vscode.Location(vscode.Uri.parse(res.uri), toVscodeRange(res.range));
      },
    })
  );

  // Commands
  const nabdaBin = () => resolveBinary('executablePath', 'nabda');
  const langFlag = () => `--lang ${getConfig().get('diagnosticLanguage') || 'both'}`;

  context.subscriptions.push(
    vscode.commands.registerCommand('nabda.runFile', async () => {
      const ed = vscode.window.activeTextEditor;
      if (!ed) return;
      await ed.document.save();
      runInTerminal(`"${nabdaBin()}" run "${ed.document.fileName}" ${langFlag()}`);
    }),
    vscode.commands.registerCommand('nabda.fixFile', async () => {
      const ed = vscode.window.activeTextEditor;
      if (!ed) return;
      await ed.document.save();
      runInTerminal(`"${nabdaBin()}" fix "${ed.document.fileName}" ${langFlag()}`);
    }),
    vscode.commands.registerCommand('nabda.checkFile', async () => {
      const ed = vscode.window.activeTextEditor;
      if (!ed) return;
      await ed.document.save();
      runInTerminal(`"${nabdaBin()}" check "${ed.document.fileName}" ${langFlag()}`);
    }),
    vscode.commands.registerCommand('nabda.buildNative', async () => {
      const ed = vscode.window.activeTextEditor;
      if (!ed) return;
      await ed.document.save();
      runInTerminal(`"${nabdaBin()}" build "${ed.document.fileName}" --target native`);
    }),
    vscode.commands.registerCommand('nabda.buildKernel', async () => {
      const ed = vscode.window.activeTextEditor;
      if (!ed) return;
      await ed.document.save();
      runInTerminal(`"${nabdaBin()}" build "${ed.document.fileName}" --target kernel`);
    }),
    vscode.commands.registerCommand('nabda.probeHardware', () => {
      runInTerminal(`"${nabdaBin()}" probe`);
    })
  );
}

function deactivate() {}

module.exports = { activate, deactivate };
