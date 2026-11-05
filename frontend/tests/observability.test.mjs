import { Buffer } from 'node:buffer';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
// Exercise the actual browser logger without adding a separate test framework.
const source = readFileSync(new URL('../src/observability.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText;
const { safeRoute, logEvent } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

test('route templates discard query, hash, identifiers and unknown paths', () => {
  assert.equal(safeRoute('/api/v1/sessions/private-session/live?after=private-cursor#invite=secret'), '/api/v1/sessions/:session/live');
  assert.equal(safeRoute('/api/v1/candidate/questions/private-question/recordings/private-recording'), '/api/v1/candidate/questions/:question/recordings/:recording');
  assert.equal(safeRoute('/api/v1/sessions/review'), '/api/v1/sessions/:session');
  assert.equal(safeRoute('/private-invitation-token'), 'unmatched');
  assert.equal(safeRoute('/api/v1/candidate/session/private-action'), 'unmatched');
  assert.equal(safeRoute('/api/v1/candidate/room'), '/api/v1/candidate/room');
});
test('production logging defaults to warnings without payloads', () => {
  const messages = []; const previous = console.warn;
  console.warn = message => messages.push(message);
  try { logEvent('warn', 'request.response', { route: safeRoute('/api/v1/jobs?secret=private'), status: 500 }); }
  finally { console.warn = previous; }
  assert.equal(messages[0].route, '/api/v1/jobs');
  assert.equal(messages[0].status, 500);
  assert.ok(!JSON.stringify(messages).includes('private'));
});

test('transport correlates requests and excludes private payloads from logs', async () => {
  const debugSource = source.replaceAll('import.meta.env', '({ DEV: true })');
  const loggerCode = ts.transpileModule(debugSource, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText;
  const loggerUrl = `data:text/javascript;base64,${Buffer.from(loggerCode).toString('base64')}`;
  const transportSource = readFileSync(new URL('../src/transport.ts', import.meta.url), 'utf8').replaceAll('import.meta.env', '({ VITE_API_BASE_URL: "" })');
  const transportCode = ts.transpileModule(transportSource, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText.replace("'./observability'", `'${loggerUrl}'`);
  const { loggedFetch } = await import(`data:text/javascript;base64,${Buffer.from(transportCode).toString('base64')}`);
  const previousFetch = globalThis.fetch; const previousDebug = console.debug; const previousError = console.error;
  const messages = []; let actualHeaders;
  console.debug = message => messages.push(message); console.error = message => messages.push(message);
  const requestId = '24ea4d79-26f1-49d7-849b-6e057b70bf46';
  try {
    globalThis.fetch = async (_url, options) => { actualHeaders = options.headers; return new Response('{}'); };
    await loggedFetch('/api/v1/candidate/answers/private-question?token=private-query', { method: 'POST', headers: { Authorization: 'Bearer private-token', 'X-Request-ID': requestId }, body: 'private-answer' });
    assert.equal(actualHeaders.get('authorization'), 'Bearer private-token');
    assert.equal(actualHeaders.get('x-request-id'), requestId);
    assert.equal(messages[0].request_id, requestId);
    assert.equal(messages[1].request_id, requestId);
    assert.equal(messages[1].status, 200);
    globalThis.fetch = async () => { throw new Error('private-exception'); };
    await assert.rejects(loggedFetch('/private-path'), /private-exception/);
    assert.ok(!JSON.stringify(messages).includes('private-'));
    assert.equal(messages.at(-1).outcome, 'network_error');
  } finally { globalThis.fetch = previousFetch; console.debug = previousDebug; console.error = previousError; }
});
