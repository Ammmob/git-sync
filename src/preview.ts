// Development-only adapter. Never included in a production desktop build.
import type { Snapshot, Task, Token } from "./types";
let state: Snapshot = {
  settings: { interval_seconds: 10, settle_seconds: 2 },
  tasks: [],
  catalog: { tokens: [], defaults: {} },
  runtime: {},
};
if (new URLSearchParams(location.search).has("fixtures")) {
  state.tasks = [
    {
      id: "demo-one",
      name: "Research manuscript",
      provider: "overleaf",
      local_dir: "D:\\Research\\paper\\source",
      remote_url: "https://git@git.overleaf.com/example",
      branch: "main",
      interval_seconds: null,
      enabled: true,
      token_id: null,
      term: { unit: "permanent", count: 1 },
      expires_at: null,
    },
    {
      id: "demo-two",
      name: "Project homepage",
      provider: "github",
      local_dir: "D:\\Research\\homepage",
      remote_url: "https://github.com/example/homepage.git",
      branch: "main",
      interval_seconds: 30,
      enabled: false,
      token_id: null,
      term: { unit: "permanent", count: 1 },
      expires_at: null,
    },
  ];
  state.runtime = {
    "demo-one": {
      status: "synced",
      message: "本地与远程已一致",
      last_success: 1789999800,
      blocked: false,
    },
    "demo-two": {
      status: "paused",
      message: "已暂停自动同步",
      last_success: null,
      blocked: false,
    },
  };
  state.catalog.tokens = [
    {
      id: "demo-token",
      provider: "overleaf",
      name: "Research account",
      masked: "olp_********8k2z",
    },
  ];
  state.catalog.defaults.overleaf = "demo-token";
}
if (new URLSearchParams(location.search).has("expiry") && state.tasks.length) {
  state.tasks[0].term = { unit: "days", count: 1 };
  state.tasks[0].expires_at = Math.floor(Date.now() / 1000) + 3;
}
export function snapshot() {
  for (const t of state.tasks)
    if (t.expires_at && t.expires_at <= Date.now() / 1000) {
      t.enabled = false;
      state.runtime[t.id] = {
        ...state.runtime[t.id],
        status: "expired",
        message: "同步期限已到，请编辑项目续期或改为永久",
      };
    }
  return structuredClone(state);
}
export function request(op: string, payload: unknown) {
  const p = payload as Record<string, any>;
  switch (op) {
    case "inspect":
      if (p.path?.includes("new-folder"))
        return { needs_setup: true, non_empty: p.path.includes("nonempty") };
      return {
        remote_url: "https://github.com/example/demo.git",
        provider: "github",
        branch: "main",
      };
    case "save_task": {
      const t = p as Task;
      if (!t.name.trim() || !t.local_dir.trim())
        throw Error("请填写项目名称和本地目录");
      const old = state.tasks.find((x) => x.id === t.id);
      if (
        !old ||
        JSON.stringify(old.term) !== JSON.stringify(t.term) ||
        p.renew_term
      ) {
        const end = new Date();
        if (t.term.unit === "days")
          end.setTime(end.getTime() + t.term.count * 86400000);
        if (t.term.unit === "months") {
          const day = end.getDate();
          end.setDate(1);
          end.setMonth(end.getMonth() + t.term.count);
          const last = new Date(
            end.getFullYear(),
            end.getMonth() + 1,
            0,
          ).getDate();
          end.setDate(Math.min(day, last));
        }
        t.expires_at =
          t.term.unit === "permanent" ? null : Math.floor(end.getTime() / 1000);
      } else t.expires_at = old.expires_at;
      if (t.expires_at && t.expires_at <= Date.now() / 1000) t.enabled = false;
      t.id ||= crypto.randomUUID();
      if (state.tasks.some((x) => x.id === t.id))
        state.tasks = state.tasks.map((x) => (x.id === t.id ? t : x));
      else state.tasks.push(t);
      state.runtime[t.id] = {
        status:
          t.expires_at && t.expires_at <= Date.now() / 1000
            ? "expired"
            : t.enabled
              ? "waiting"
              : "paused",
        message: "",
        last_success: null,
        blocked: false,
      };
      return t.id;
    }
    case "pause":
    case "resume":
    case "sync": {
      const t = state.tasks.find((t) => t.id === p.id)!;
      if (op !== "pause" && t.expires_at && t.expires_at <= Date.now() / 1000)
        throw Error("期限已到，请先编辑项目续期");
      if (op !== "sync") t.enabled = op === "resume";
      state.runtime[t.id] = {
        ...state.runtime[t.id],
        status: t.enabled ? "synced" : "paused",
        message: op === "pause" ? "已暂停自动同步" : "本地与远程已一致",
      };
      return;
    }
    case "batch_tasks": {
      const ids = p.all ? state.tasks.map((t) => t.id) : p.ids;
      let processed = 0;
      const skipped: { id: string; name: string; reason: string }[] = [];
      for (const id of new Set<string>(ids)) {
        if (!state.tasks.some((t) => t.id === id)) continue;
        try {
          request(p.action, { id });
          processed++;
        } catch (e) {
          skipped.push({
            id,
            name: state.tasks.find((t) => t.id === id)!.name,
            reason: String(e),
          });
        }
      }
      return { processed, skipped };
    }
    case "remove_task":
      state.tasks = state.tasks.filter((t) => t.id !== p.id);
      delete state.runtime[p.id];
      return;
    case "save_settings":
      state.settings = {
        interval_seconds: p.interval_seconds,
        settle_seconds: p.settle_seconds,
      };
      return;
    case "add_token": {
      const t: Token = {
        id: crypto.randomUUID(),
        name: p.name,
        provider: p.provider,
        masked:
          p.token.length < 10
            ? "*".repeat(p.token.length)
            : p.token.slice(0, 4) + "********" + p.token.slice(-4),
      };
      state.catalog.tokens.push(t);
      if (p.make_default) state.catalog.defaults[t.provider] = t.id;
      return;
    }
    case "default_token":
      state.catalog.defaults[p.provider as "github"] = p.id;
      return;
    case "remove_token":
      if (state.tasks.some((t) => t.token_id === p.id))
        throw Error("请先更换项目使用的 Token");
      state.catalog.tokens = state.catalog.tokens.filter((t) => t.id !== p.id);
      for (const k of ["github", "overleaf"] as const)
        if (state.catalog.defaults[k] === p.id)
          state.catalog.defaults[k] = null;
      return;
    case "logs":
      return "暂无同步记录";
    case "open_folder":
      return;
    default:
      throw Error("Unsupported preview operation");
  }
}
