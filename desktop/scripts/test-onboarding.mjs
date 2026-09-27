// Bundle the actual React component with a test-only IPC replacement.
// The test bundle lives under node_modules and never enters the Tauri app.
import { build } from "esbuild";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const desktop = fileURLToPath(new URL("../", import.meta.url));
const cache = path.join(desktop, "node_modules/.cache");
await mkdir(cache, { recursive: true });
const dir = await mkdtemp(path.join(cache, "strkd-onboarding-"));
try {
  const outfile = path.join(dir, "onboarding.test.cjs");
  await build({
    absWorkingDir: desktop,
    entryPoints: ["tests/onboarding.test.tsx"],
    outfile,
    bundle: true,
    platform: "node",
    format: "cjs",
    external: ["jsdom", "react", "react-dom"],
    define: { "process.env.NODE_ENV": '"test"' },
    plugins: [{ name: "test-only-ipc", setup(b) {
      b.onResolve({ filter: /^\.\.\/api$/ }, () => ({
        path: path.join(desktop, "tests/mock-api.ts"),
      }));
    } }],
  });
  const result = spawnSync(process.execPath, ["--test", outfile], { stdio: "inherit", timeout: 30_000 });
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
} finally {
  await rm(dir, { recursive: true, force: true });
}
