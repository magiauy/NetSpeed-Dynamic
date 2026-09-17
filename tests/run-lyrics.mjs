import { build } from 'esbuild';
import { createRequire } from 'node:module';
import { runInThisContext } from 'node:vm';
const result = await build({ entryPoints: ['tests/lyrics.test.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external' });
runInThisContext(`(function(require){${result.outputFiles[0].text}\n})`, { filename: 'lyrics-tests.cjs' })(createRequire(import.meta.url));
