// Thin wrappers over the Tauri IPC commands exposed by src-tauri/src/lib.rs.
// The companion's own UI talks to the core over IPC; external agents/apps use
// the loopback JSON-RPC service instead.

import { invoke, Channel } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Status {
  locked: boolean;
  needs_onboarding: boolean;
  network: string;
  version: string;
  service_url: string;
  accounts: number;
  node_configured: boolean;
}

export interface Account {
  domain: "user" | "agent";
  index: number;
  address: string;
  label: string;
  owner_client_id?: string;
}

export interface LogEntry {
  ts_unix_ms: number;
  method: string;
  client?: string | null;
  network?: string | null;
  decision: string;
  outcome: string;
  error_code?: number | null;
  latency_ms: number;
  params_json?: string | null;
  result_json?: string | null;
}

export interface ApprovalRequest {
  id: number;
  client_label: string;
  method: string;
  summary: string;
}

export interface Settings {
  sepolia_rpc: string;
  mainnet_rpc: string;
  auto_lock_minutes: number;
}

export interface ClientInfo {
  id: string;
  label: string;
  kind: "agent" | "app";
  granted_until: number | null;
}

// ── On-device proving companion ─────────────────────────────────────────────

export interface ProverStatus {
  prover: string; // backend kind: "native" | "remote" | "starknet-rpc"
  ready: boolean;
  version: string;
  service_url: string;
}

export type JobStatus = "queued" | "proving" | "succeeded" | "failed";

export interface Activity {
  seq: number;
  job_id: string;
  status: JobStatus;
  started_at_ms: number;
  label?: string;
}

export interface ProverNetworkConfig {
  rpc_url: string;
  prover_url: string;
  prover_api_key: string;
}

export interface ProverSettings {
  mainnet: ProverNetworkConfig;
  testnet: ProverNetworkConfig;
  /** "" (env/default), "native", "remote", or "starknet-rpc". Applied on restart. */
  prover_backend: string;
}

export interface StorageStats {
  records: number;
  bytes: number;
}

export interface ProofSummary {
  job_id: string;
  label?: string;
  network: string;
  created_at_ms: number;
  prove_ms: number;
  status: JobStatus;
  proof_bytes: number;
  error?: string;
}

export interface ProofRecord {
  job_id: string;
  label?: string;
  network: string;
  created_at_ms: number;
  prove_ms: number;
  status: JobStatus;
  payload: unknown;
  proof?: { proof?: string; proof_facts?: string[]; l2_to_l1_messages?: unknown[] };
  error?: string;
}

export interface PrivacyNetwork {
  pool_address: string; discovery_url: string; screening_policy: "required" | "pool_enforced";
  deposit_prover?: "configured" | "starkscan";
}
export interface PrivacySettings { mainnet: PrivacyNetwork; testnet: PrivacyNetwork }
export interface PrivacyRequest {
  account: string; chain_id: string; mode: "status" | "balances" | "prepare";
  operation?: "register" | "deposit" | "transfer" | "withdraw"; amount?: string; recipient?: string;
}
export interface PrivacyStatus {
  registered: boolean; registration_mature: boolean; public_balance: string; pool_fee: string; proof_base: number;
  shielded_balance?: string; spendable_balance?: string; notes?: number;
  deposit_screening?: "unverified" | "signature_missing" | "pool_enforced";
  hosted_prover?: {
    reachable: boolean; local_attempts?: number; local_limit?: number; local_remaining?: number;
    resets_at?: string; retry_after_seconds?: number; pending?: {job_id: string | null; status: string}[];
  };
}
export interface PrivacyReview {
  review_id: string; operation: string; amount: string; recipient: string;
  account: string; chain_id: string; pool_fee: string; max_network_fee: string;
  proof_base: number; screening_attached: boolean; screening_policy: string; adapter: string; warnings: string[];
}
export type PrivacyProgressStage = "preparing" | "proving" | "checking_fees" | "submitting";
export interface PrivacySubmission { transaction_hash: string; status: string; chain_id: string }
export interface PrivacyReceipt {
  transaction_hash: string; execution_status: string | null; finality_status: string | null;
  block_number: number | null; actual_fee: { amount: string; unit: string } | null;
}

export const api = {
  privacySettings: () => invoke<{ settings: PrivacySettings; runtime_ready: boolean }>("privacy_settings"),
  setPrivacySettings: (settings: PrivacySettings) => invoke<void>("set_privacy_settings", { settings }),
  privacyStatus: (request: PrivacyRequest) => invoke<PrivacyStatus>("privacy_run", { request }),
  privacyPrepare: (request: PrivacyRequest) => invoke<PrivacyReview>("privacy_run", { request }),
  privacyExecute: (request: PrivacyRequest, limits: {max_pool_fee:string;max_network_fee:string}, onProgress: (stage:PrivacyProgressStage)=>void) => {
    const onProgressChannel=new Channel<PrivacyProgressStage>();
    onProgressChannel.onmessage=onProgress;
    return invoke<PrivacySubmission>("privacy_execute", {request,limits,onProgress:onProgressChannel});
  },
  privacySubmit: (reviewId: string) => invoke<PrivacySubmission>("privacy_submit", { reviewId }),
  privacyReceipt: (transactionHash: string, chainId: string) => invoke<PrivacyReceipt>("privacy_receipt", { transactionHash, chainId }),
  privacyHistory: (account: string, chainId: string) => invoke<PrivacySubmission[]>("privacy_history", { account, chainId }),
  status: () => invoke<Status>("status"),
  generate: (wordCount: number) => invoke<string>("generate", { wordCount }),
  import: (phrase: string) => invoke<void>("import", { phrase }),
  cancelSetup: () => invoke<void>("cancel_setup"),
  finalizeSetup: (passphrase: string) => invoke<void>("finalize_setup", { passphrase }),
  unlock: (passphrase: string) => invoke<void>("unlock", { passphrase }),
  lock: () => invoke<void>("lock"),
  setNetwork: (network: "mainnet" | "testnet") => invoke<void>("set_network", { network }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  listAccounts: () => invoke<Account[]>("list_accounts"),
  addUserAccount: (label: string) => invoke<Account>("add_user_account", { label }),
  deployStatus: (address: string) => invoke<boolean | null>("deploy_status", { address }),
  // STRK balance in fri (string, to avoid JS precision loss); null when no node.
  balance: (address: string) => invoke<string | null>("balance", { address }),
  deployAccount: (address: string) => invoke<{ transaction_hash: string }>("deploy_account", { address }),
  recentLog: (limit: number) => invoke<LogEntry[]>("recent_log", { limit }),
  listClients: () => invoke<ClientInfo[]>("list_clients"),
  grantPermission: (clientId: string, days: number) =>
    invoke<void>("grant_permission", { clientId, days }),
  revokePermission: (clientId: string) => invoke<void>("revoke_permission", { clientId }),
  respondApproval: (id: number, approved: boolean) =>
    invoke<void>("respond_approval", { id, approved }),

  // On-device proving companion (IPC-only; agents prove via companion_prove on
  // the loopback service). Settings carry an API key, so they never leave IPC.
  proverStatus: () => invoke<ProverStatus>("prover_status"),
  proofActivity: () => invoke<Activity[]>("proof_activity"),
  getProverSettings: () => invoke<ProverSettings>("get_prover_settings"),
  setProverSettings: (settings: ProverSettings) =>
    invoke<void>("set_prover_settings", { settings }),
  storageStats: () => invoke<StorageStats>("storage_stats"),
  clearStorage: () => invoke<number>("clear_storage"),
  listProofs: () => invoke<ProofSummary[]>("list_proofs"),
  proofDetail: (jobId: string) => invoke<ProofRecord | null>("proof_detail", { jobId }),
};

/// Subscribe to approval prompts pushed from the service. Returns an unlisten fn.
export function onApprovalRequest(cb: (req: ApprovalRequest) => void): Promise<UnlistenFn> {
  return listen<ApprovalRequest>("approval-request", (e) => cb(e.payload));
}
