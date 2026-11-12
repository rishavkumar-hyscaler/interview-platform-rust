import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
test('a rejected observation does not block later flags; uploaded assets are checkpointed', async () => {
  const records = new Map();
  const sent = [];
  const uploads = [];
  let tick;
  let notice = '';
  const observation = (id) => ({
    id,
    kind: 'face_missing',
    client_time: Date.now(),
    measurements: { duration_ms: 5000 },
  });
  for (const id of ['bad', 'good'])
    records.set(id, {
      id,
      session: 'session',
      observation: observation(id),
      assets: [],
      created: Date.now(),
      accepted: false,
    });
  records.set('assets', {
    id: 'assets',
    session: 'session',
    observation: observation('assets'),
    assets: [
      { id: 'first', blob: new Blob(['x']) },
      { id: 'second', blob: new Blob(['y']) },
    ],
    created: Date.now(),
    accepted: true,
  });
  // Model IndexedDB transactions, including structured cloning on reads/writes.
  const indexedDB = {
    open() {
      const opening = {};
      queueMicrotask(() => {
        opening.result = {
          close() {},
          transaction() {
            const tx = {
              objectStore() {
                return {
                  getAll() {
                    const result = {
                      result: structuredClone([...records.values()]),
                    };
                    setImmediate(() => tx.oncomplete());
                    return result;
                  },
                  put(record) {
                    records.set(record.id, structuredClone(record));
                    const result = {};
                    setImmediate(() => tx.oncomplete());
                    return result;
                  },
                  delete(id) {
                    records.delete(id);
                    const result = {};
                    setImmediate(() => tx.oncomplete());
                    return result;
                  },
                };
              },
            };
            return tx;
          },
        };
        opening.onsuccess();
      });
      return opening;
    },
  };
  class ApiError extends Error {
    constructor(message, status) {
      super(message);
      this.status = status;
    }
  }
  const modules = {
    '../api': {
      candidateApi: () => ({
        events: async (events) => {
          sent.push(events[0].id);
          if (events[0].id === 'bad') throw new ApiError('invalid', 400);
        },
      }),
    },
    '../../../api': { ApiError },
    '../../../observability': { startTask: () => () => {} },
    '../../../transport': {
      loggedFetch: async (url) => {
        uploads.push(url);
        return { ok: !url.endsWith('/second'), status: 503 };
      },
    },
  };
  const source = readFileSync(
    new URL(
      '../src/features/interview/proctoring/evidence.ts',
      import.meta.url,
    ),
    'utf8',
  );
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.CommonJS,
    },
  }).outputText;
  const exports = {};
  vm.runInNewContext(compiled, {
    exports,
    require: (key) => modules[key],
    indexedDB,
    Date,
    structuredClone,
    AbortSignal,
    crypto,
    Blob,
    setInterval: (callback) => {
      tick = callback;
      return 1;
    },
    clearInterval() {},
    window: { addEventListener() {}, removeEventListener() {} },
  });
  const queue = exports.evidenceQueue(
    'token',
    'session',
    (message) => (notice = message),
  );
  const settle = async () => {
    for (let i = 0; i < 30; i++)
      await new Promise((resolve) => setImmediate(resolve));
  };
  await settle();
  assert.ok(sent.includes('good'));
  assert.equal(records.has('good'), false);
  assert.equal(records.has('bad'), true);
  assert.deepEqual(
    records.get('assets').assets.map((asset) => asset.id),
    ['second'],
  );
  assert.match(notice, /pending/);
  tick();
  await settle();
  assert.equal(uploads.filter((url) => url.endsWith('/first')).length, 1);
  for (let i = 0; i < 20; i++)
    records.set(`full-${i}`, {
      id: `full-${i}`,
      session: 'session',
      created: Date.now(),
    });
  await assert.rejects(
    queue.enqueue(observation('overflow'), [
      { id: 'overflow-image', blob: new Blob(['image']) },
    ]),
    /attachment was not saved/,
  );
  assert.ok(
    sent.includes('overflow'),
    'a full media buffer must still preserve the observation',
  );
  assert.match(notice, /buffer full/);
  queue.stop();
});
test('unavailable local storage still sends observations and reports unsaved attachments', async () => {
  const sent = [];
  let notice = '';
  const exports = {};
  const source = readFileSync(
    new URL(
      '../src/features/interview/proctoring/evidence.ts',
      import.meta.url,
    ),
    'utf8',
  );
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.CommonJS,
    },
  }).outputText;
  const modules = {
    '../api': {
      candidateApi: () => ({
        events: async (events) => sent.push(events[0].id),
      }),
    },
    '../../../api': { ApiError: class extends Error {} },
    '../../../transport': {},
    '../../../observability': {},
  };
  vm.runInNewContext(compiled, {
    exports,
    require: (key) => modules[key],
    indexedDB: {
      open() {
        throw new Error('storage blocked');
      },
    },
    setInterval() {},
    clearInterval() {},
    window: { addEventListener() {}, removeEventListener() {} },
  });
  const queue = exports.evidenceQueue(
    'token',
    'session',
    (message) => (notice = message),
  );
  const event = { id: 'flag', kind: 'face_missing', client_time: 1 };
  await queue.enqueue(event, []);
  await assert.rejects(
    queue.enqueue(event, [{ id: 'image', blob: new Blob(['x']) }]),
    /attachment was not saved/,
  );
  assert.deepEqual(sent, ['flag', 'flag']);
  assert.match(notice, /local evidence recovery unavailable/);
  queue.stop();
});
