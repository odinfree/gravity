import { build } from 'esbuild';
import { mkdir } from 'node:fs/promises';
await mkdir('../desktop/src-tauri/resources/privacy', { recursive: true });
const result=await build({entryPoints:['entry.mjs'], bundle:true, platform:'node', target:'node24', metafile:true,
  format:'cjs', outfile:'../desktop/src-tauri/resources/privacy/worker.cjs',
  minify:false, legalComments:'eof'});
// The pinned SDK declares development tooling as production dependencies.
// Its devnet/archive extractor must never enter the shipped wallet worker.
for(const output of Object.values(result.metafile.outputs)) {
  for(const [name,input] of Object.entries(output.inputs)) {
    if(input.bytesInOutput>0 && /node_modules\/(starknet-devnet|decompress)\//.test(name))
      throw new Error('Development-only archive extraction dependency entered wallet bundle');
  }
}
