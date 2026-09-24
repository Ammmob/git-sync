export type Provider = "github" | "overleaf";
export interface SyncTerm {
  unit: "days" | "months" | "permanent";
  count: number;
}
export interface Task {
  id: string;
  name: string;
  provider: Provider;
  local_dir: string;
  remote_url: string;
  branch: string;
  interval_seconds: number | null;
  enabled: boolean;
  token_id: string | null;
  term: SyncTerm;
  expires_at: number | null;
}
export interface Runtime {
  status: string;
  message: string;
  last_success: number | null;
  blocked: boolean;
}
export interface Token {
  id: string;
  provider: Provider;
  name: string;
  masked: string;
}
export interface Settings {
  interval_seconds: number;
  settle_seconds: number;
}
export interface Snapshot {
  settings: Settings;
  tasks: Task[];
  catalog: {
    tokens: Token[];
    defaults: Partial<Record<Provider, string | null>>;
  };
  runtime: Record<string, Runtime>;
}
