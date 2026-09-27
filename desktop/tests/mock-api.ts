// Deliberately invalid recovery words. Never talks to a wallet or real IPC.
export const calls: { method: string; value?: unknown }[] = [];
export const api = {
  generate: async (count: number) => {
    calls.push({ method: "generate", value: count });
    return Array.from({ length: count }, (_, i) => `fixtureword${i + 1}`).join(" ");
  },
  import: async (phrase: string) => { calls.push({ method: "import", value: phrase }); },
  cancelSetup: async () => { calls.push({ method: "cancel" }); },
  finalizeSetup: async (password: string) => { calls.push({ method: "finalize", value: password }); },
};
