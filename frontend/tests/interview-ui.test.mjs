import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import vm from 'node:vm';
import ts from 'typescript';
import { renderToStaticMarkup } from 'react-dom/server';
import { createElement } from 'react';
const require = createRequire(import.meta.url);
function load(file, globals = {}) {
  const source = readFileSync(new URL(`../src/features/interview/${file}`, import.meta.url), 'utf8');
  const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } }).outputText;
  const exports = {};
  vm.runInNewContext(compiled, { exports, require, ...globals });
  return exports;
}
test('questions format lists, inline code, bold and fenced code without interpreting HTML', () => {
  const { QuestionContent } = load('QuestionContent.tsx');
  const html = renderToStaticMarkup(createElement(QuestionContent, {content: 'Use **transactions** and `BEGIN`.\n\n- First step\n- Second step\n\n1. Validate\n2. Save\n\n```js\nconst tag = "<script>";\n```\n<img src=x onerror=alert(1)>'}));
  assert.match(html, /<strong>transactions<\/strong>/);
  assert.match(html, /<code>BEGIN<\/code>/);
  assert.match(html, /<ul><li>First step<\/li><li>Second step<\/li><\/ul>/);
  assert.match(html, /<ol start="1">/);
  assert.match(html, /<pre><code>/);
  assert.ok(!html.includes('<script>'));
  assert.ok(!html.includes('<img'));
});
test('fullscreen is entered from the action, is idempotent and reports denied access', async () => {
  let calls = 0;
  const document = {fullscreenElement:null,fullscreenEnabled:true,documentElement:{requestFullscreen:async()=>{calls++;document.fullscreenElement={};}}};
  const { enterInterviewFullscreen } = load('fullscreen.ts', {document});
  await enterInterviewFullscreen(); await enterInterviewFullscreen(); assert.equal(calls, 1);
  document.fullscreenElement=null;
  document.documentElement.requestFullscreen=async()=>{throw new Error('denied');};
  await assert.rejects(enterInterviewFullscreen(), /Fullscreen was blocked/);
  document.fullscreenEnabled=false;
  await assert.rejects(enterInterviewFullscreen(), /Fullscreen is required/);
});
test('camera preview ignores stale play failures and clears a recovered warning', async () => {
  const source = readFileSync(new URL('../src/features/interview/InterviewMedia.tsx', import.meta.url), 'utf8');
  const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } }).outputText;
  const preview = new EventTarget(); const attempts = [];
  preview.play = () => new Promise((resolve, reject) => attempts.push({resolve, reject}));
  const refs = []; const states = []; let effects = []; let refIndex = 0; let stateIndex = 0;
  const react = {
    useRef(value) { const index = refIndex++; return refs[index] ??= {current:index === 0 ? preview : value}; },
    useState(value) { const index = stateIndex++; if (!(index in states)) states[index] = value; return [states[index], next => {states[index] = next;}]; },
    useEffect(callback, deps) { effects.push({callback, deps}); },
  };
  const exports = {};
  vm.runInNewContext(compiled, {exports, DOMException, MediaStream:class {constructor(tracks){this.tracks=tracks;}}, require(name) {
    if (name === 'react') return react;
    if (name.startsWith('./proctoring/')) return {};
    return require(name);
  }});
  const track = {readyState:'live'};
  const devices = {camera:'enabled',microphone:'waiting',revision:1,stream:{current:{getVideoTracks:()=>[track]}}};
  const render = () => {effects=[];refIndex=0;stateIndex=0;exports.InterviewMedia({token:'test',sessionId:'test',devices});return effects[1];};
  const first = render();const cleanup = first.callback();
  devices.microphone='enabled';devices.revision++;
  const second = render();
  assert.deepEqual(second.deps,first.deps,'Microphone changes must not restart preview');
  // React StrictMode cleans up and restarts an effect before old play() settles.
  cleanup();const cleanupSecond = second.callback();
  attempts[0].reject(new DOMException('old stream','NotAllowedError'));
  await Promise.resolve();await Promise.resolve();
  assert.equal(states[3],'');
  attempts[1].reject(new DOMException('permission denied','NotAllowedError'));
  await Promise.resolve();await Promise.resolve();
  assert.match(states[3],/playback was blocked/);
  preview.dispatchEvent(new Event('playing'));
  assert.equal(states[3],'');
  cleanupSecond();const cleanupThird=second.callback();
  attempts[2].reject(new DOMException('stream replaced','AbortError'));
  await Promise.resolve();await Promise.resolve();
  assert.equal(states[3],'');
  cleanupThird();
});
