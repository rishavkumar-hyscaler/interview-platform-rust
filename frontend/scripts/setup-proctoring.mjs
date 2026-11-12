import { mkdir, cp, readFile, writeFile, rename, rm } from 'node:fs/promises';
import { build } from 'esbuild';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
const root = new URL('../', import.meta.url);
const destination = new URL('public/proctoring/', root);
await mkdir(destination, { recursive: true });
for (const kind of ['vision', 'audio']) {
  await cp(
    new URL(`node_modules/@mediapipe/tasks-${kind}/wasm/`, root),
    new URL(`${kind}/`, destination),
    { recursive: true },
  );
}
const manifest = JSON.parse(
  await readFile(new URL('proctoring-models.json', import.meta.url), 'utf8'),
);
for (const model of manifest) {
  const target = new URL(model.file, destination);
  const valid = async () => {
    try {
      return (
        createHash('sha256')
          .update(await readFile(target))
          .digest('hex') === model.sha256
      );
    } catch {
      return false;
    }
  };
  if (await valid()) continue;
  console.info(`Downloading proctoring model: ${model.file}`);
  const response = await fetch(model.url, {
    signal: AbortSignal.timeout(120000),
  });
  if (!response.ok)
    throw new Error(`Model download failed: ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  const checksum = createHash('sha256').update(bytes).digest('hex');
  if (model.sha256 && checksum !== model.sha256)
    throw new Error(`Model checksum mismatch: ${model.file}`);
  const temporary = new URL(`${model.file}.tmp`, destination);
  await writeFile(temporary, bytes);
  await rename(temporary, target);
  if (!model.sha256) console.info(`${model.file} sha256: ${checksum}`);
}
await rm(new URL('.setup-incomplete', destination), { force: true });

await build({
  entryPoints: [
    fileURLToPath(
      new URL('src/features/interview/proctoring/inference.worker.ts', root),
    ),
  ],
  outfile: fileURLToPath(new URL('inference-worker.js', destination)),
  bundle: true,
  format: 'iife',
  platform: 'browser',
  target: 'es2022',
  logLevel: 'warning',
});
await cp(
  new URL('src/features/interview/proctoring/pcm-worklet.js', root),
  new URL('pcm-worklet.js', destination),
);
