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
