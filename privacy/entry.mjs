import { main } from './worker.mjs';
main().catch(() => { process.exitCode=1; });
