import { useEffect, useState, type ReactNode, type FormEvent } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import {
  ArrowLeftRight,
  Layers,
  Settings2,
  Plus,
  Search,
  RefreshCw,
  Play,
  Pause,
  FolderOpen,
  Pencil,
  Trash2,
  X,
  Check,
  ChevronRight,
  Clock3,
  KeyRound,
  FileText,
  ArrowUpRight,
  CircleHelp,
  Minus,
  AlertCircle,
  ArrowUpDown,
  ArrowUp,
  ArrowDown,
  Monitor,
  Sun,
  Moon,
  Palette,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import * as api from "./api";
import type { Provider, Snapshot, Task, Token, Settings, Runtime } from "./types";
const providers: Provider[] = ["overleaf", "github"];
const names = { overleaf: "Overleaf", github: "GitHub" };
const labels: Record<string, string> = {
  paused: "已暂停",
  expired: "已到期",
  waiting: "等待同步",
  syncing: "正在同步",
  synced: "已同步",
  error: "同步失败",
  blocked: "需要处理",
};
const blank: Snapshot = {
  tasks: [],
  settings: { interval_seconds: 10, settle_seconds: 2 },
  catalog: { tokens: [], defaults: {} },
  runtime: {},
};
type SortKey =
  | "provider"
  | "name"
  | "branch"
  | "interval"
  | "status"
  | "expires"
  | "last_success";
type Theme = "system" | "light" | "dark";
const themeKey = "git-sync-theme";
function readTheme(): Theme {
  const value = localStorage.getItem(themeKey);
  return value === "light" || value === "dark" || value === "system"
    ? value
    : "system";
}
function applyTheme(theme: Theme) {
  const dark =
    theme === "dark" ||
    (theme === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}
function compare(a: string | number, b: string | number) {
  if (typeof a === "number" && typeof b === "number") return a - b;
  const sa = String(a).toLowerCase(),
    sb = String(b).toLowerCase();
  return sa < sb ? -1 : sa > sb ? 1 : 0;
}
function sortValue(
  t: Task,
  rt: Runtime | undefined,
  key: SortKey,
  fallbackInterval: number,
): string | number {
  switch (key) {
    case "provider":
      return names[t.provider];
    case "name":
      return t.name;
    case "branch":
      return t.branch;
    case "interval":
      return t.interval_seconds ?? fallbackInterval;
    case "status":
      return labels[rt?.status || "paused"];
    case "expires":
      return t.expires_at ?? Number.MAX_SAFE_INTEGER;
    case "last_success":
      return rt?.last_success ?? -1;
  }
}
function Platform({ provider }: { provider: Provider }) {
  return (
    <img
      className="platform-icon"
      src={`/${provider}.png`}
      alt={names[provider]}
    />
  );
}
function Modal({
  title,
  description,
  children,
  onClose,
  wide = false,
}: {
  title: string;
  description?: string;
  children: ReactNode;
  onClose: () => void;
  wide?: boolean;
}) {
  return (
    <Dialog.Root open onOpenChange={(v) => !v && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="overlay" />
        <Dialog.Content
          className={`dialog ${wide ? "wide" : ""}`}
          aria-describedby={description ? "dialog-description" : undefined}
        >
          <div className="dialog-heading">
            <div>
              <Dialog.Title>{title}</Dialog.Title>
              {description && (
                <Dialog.Description id="dialog-description">
                  {description}
                </Dialog.Description>
              )}
            </div>
            <Dialog.Close className="icon-button" aria-label="关闭">
              <X size={19} />
            </Dialog.Close>
          </div>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
function Segments({
  value,
  onChange,
}: {
  value: Provider;
  onChange: (v: Provider) => void;
}) {
  return (
    <div className="segments" role="tablist" aria-label="平台">
      {providers.map((p) => (
        <button
          type="button"
          role="tab"
          aria-selected={value === p}
          className={value === p ? "active" : ""}
          onClick={() => onChange(p)}
          key={p}
        >
          <Platform provider={p} />
          <span>{names[p]}</span>
        </button>
      ))}
    </div>
  );
}
function SortHeader({
  label,
  sortKey,
  sort,
  onSort,
  className,
}: {
  label: string;
  sortKey: SortKey;
  sort: { key: SortKey; dir: "asc" | "desc" } | null;
  onSort: (key: SortKey) => void;
  className?: string;
}) {
  const active = sort?.key === sortKey;
  const dir = active ? sort!.dir : undefined;
  const Icon = !active ? ArrowUpDown : dir === "asc" ? ArrowUp : ArrowDown;
  return (
    <th
      className={className}
      aria-sort={
        dir === "asc" ? "ascending" : dir === "desc" ? "descending" : undefined
      }
    >
      <button
        type="button"
        className={`sort-button${active ? " active" : ""}`}
        onClick={() => onSort(sortKey)}
        aria-label={`按${label}排序`}
      >
        <span>{label}</span>
        <Icon size={13} aria-hidden />
      </button>
    </th>
  );
}
function ErrorText({ text }: { text: string }) {
  return text ? (
    <div className="form-error" role="alert">
      <AlertCircle size={17} />
      <span>{text}</span>
    </div>
  ) : null;
}
function date(value: number | null | undefined) {
  return value
    ? new Date(value * 1000).toLocaleString("zh-CN", {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
        hour12: false,
      })
    : "尚未同步";
}
function termDate(t: Task) {
  return t.expires_at
    ? new Date(t.expires_at * 1000).toLocaleDateString("zh-CN", {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
      })
    : "永久";
}
export default function App() {
  const [data, setData] = useState(blank),
    [page, setPage] = useState<"projects" | "settings">("projects"),
    [filter, setFilter] = useState<"all" | Provider>("all"),
    [search, setSearch] = useState(""),
    [selected, setSelected] = useState<string | null>(null),
    [editing, setEditing] = useState<Task | null | undefined>(),
    [notice, setNotice] = useState(""),
    [confirm, setConfirm] = useState<{
      title: string;
      text: string;
      action: () => Promise<void>;
    } | null>(null),
    [logs, setLogs] = useState<string | null>(null),
    [closing, setClosing] = useState(false),
    [pending, setPending] = useState(false),
    [sort, setSort] = useState<{ key: SortKey; dir: "asc" | "desc" } | null>(
      null,
    ),
    [theme, setTheme] = useState<Theme>(() => readTheme());
  const [checked, setChecked] = useState<string[]>([]);
  async function refresh() {
    try {
      setData(await api.snapshot());
    } catch (e) {
      setNotice(String(e));
    }
  }
  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 1000);
    return () => clearInterval(timer);
  }, []);
  useEffect(() => {
    if (api.preview) return;
    const promise = listen("close-requested", () => setClosing(true));
    return () => {
      void promise.then((f) => f());
    };
  }, []);
  useEffect(() => {
    localStorage.setItem(themeKey, theme);
    applyTheme(theme);
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme(theme);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [theme]);
  async function act(op: string, payload: unknown = {}) {
    setPending(true);
    try {
      await api.request(op, payload);
      await refresh();
    } catch (e) {
      setNotice(String(e));
      throw e;
    } finally {
      setPending(false);
    }
  }
  const task = data.tasks.find((t) => t.id === selected),
    runtime = task ? data.runtime[task.id] : undefined;
  const filtered = data.tasks.filter(
    (t) =>
      (filter === "all" || t.provider === filter) &&
      `${t.name} ${t.local_dir}`.toLowerCase().includes(search.toLowerCase()),
  );
  const visible = sort
    ? [...filtered].sort((a, b) => {
        const av = sortValue(
          a,
          data.runtime[a.id],
          sort.key,
          data.settings.interval_seconds,
        );
        const bv = sortValue(
          b,
          data.runtime[b.id],
          sort.key,
          data.settings.interval_seconds,
        );
        const cmp = compare(av, bv);
        return sort.dir === "asc" ? cmp : -cmp;
      })
    : filtered;
  function nav(value: "all" | Provider) {
    setChecked([]);
    setFilter(value);
    setPage("projects");
  }
  function toggleSort(key: SortKey) {
    setSort((s) =>
      s?.key === key
        ? s.dir === "asc"
          ? { key, dir: "desc" }
          : null
        : { key, dir: "asc" },
    );
  }
  const chosen = checked.filter((id) => filtered.some((t) => t.id === id));
  async function batch(
    action: "pause" | "resume" | "remove_task",
    all = false,
  ) {
    setPending(true);
    try {
      const result = await api.request<{
        processed: number;
        skipped: { id: string; name: string; reason: string }[];
      }>("batch_tasks", { action, all, ids: chosen });
      await refresh();
      setChecked(result.skipped.map((t) => t.id));
      if (result.skipped.length)
        setNotice(
          result.processed +
            " 个项目已处理；" +
            result.skipped.map((t) => t.name + "：" + t.reason).join("；"),
        );
    } catch (e) {
      setNotice(String(e));
      throw e;
    } finally {
      setPending(false);
    }
  }
  function safely(p: Promise<unknown>) {
    void p.catch(() => {});
  }
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">
            <ArrowLeftRight size={23} />
          </div>
          <span>
            Git Sync<small>保持工作同步</small>
          </span>
        </div>
        <div className="nav-section">工作空间</div>
        <nav>
          <button
            className={
              page === "projects" && filter === "all" ? "nav active" : "nav"
            }
            onClick={() => nav("all")}
          >
            <Layers size={18} />
            <span>全部项目</span>
            <em>{data.tasks.length}</em>
          </button>
          {providers.map((p) => (
            <button
              key={p}
              className={
                page === "projects" && filter === p ? "nav active" : "nav"
              }
              onClick={() => nav(p)}
            >
              <Platform provider={p} />
              <span>{names[p]}</span>
              <em>{data.tasks.filter((t) => t.provider === p).length}</em>
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <button
            className={page === "settings" ? "nav active" : "nav"}
            onClick={() => setPage("settings")}
          >
            <Settings2 size={18} />
            <span>全局设置</span>
          </button>
          <div className="version">
            <span className="tiny-dot" />
            Git Sync <span>0.1.0</span>
          </div>
        </div>
      </aside>
      <main>
        <div className="topbar">
          <span>
            工作空间 <ChevronRight size={13} />{" "}
            {page === "settings"
              ? "全局设置"
              : filter === "all"
                ? "全部项目"
                : names[filter]}
          </span>
          <span className="topbar-hint">
            <span
              className={`tiny-dot ${data.tasks.some((t) => t.enabled) ? "green" : ""}`}
            />
            {data.tasks.some((t) => t.enabled)
              ? "自动同步已启用"
              : "自动同步待命"}
          </span>
        </div>
        <div className="main-content">
          {page === "projects" ? (
            <>
              <header className="page-heading">
                <div>
                  <div className="eyebrow">YOUR WORK, IN SYNC</div>
                  <h1>
                    {filter === "all" ? "同步项目" : `${names[filter]} 项目`}
                  </h1>
                  <p>管理本地与远程仓库，让每一次修改保持一致。</p>
                </div>
                <button
                  className="button primary"
                  onClick={() => setEditing(null)}
                >
                  <Plus size={17} />
                  新增同步
                </button>
              </header>
              <div className="overview">
                <span>
                  <span className="tiny-dot green" />
                  {data.tasks.filter((t) => t.enabled).length} 个自动同步
                </span>
                <i />
                <span>
                  <Pause size={13} />
                  {data.tasks.filter((t) => !t.enabled).length} 个已暂停
                </span>
                {Object.values(data.runtime).some(
                  (r) => r.blocked || r.status === "error",
                ) && (
                  <>
                    <i />
                    <span className="danger-text">
                      <AlertCircle size={14} />
                      有任务需要检查
                    </span>
                  </>
                )}
              </div>
              <section className="panel projects-panel">
                <div className="panel-toolbar">
                  <div className="panel-title">
                    项目列表 <span className="count">{filtered.length}</span>
                  </div>
                  <div className="search">
                    <Search size={16} />
                    <input
                      aria-label="搜索项目"
                      placeholder="搜索项目或本地路径"
                      value={search}
                      onChange={(e) => {
                        setChecked([]);
                        setSearch(e.target.value);
                      }}
                    />
                    {search && (
                      <button
                        aria-label="清空搜索"
                        onClick={() => {
                          setChecked([]);
                          setSearch("");
                        }}
                      >
                        <X size={14} />
                      </button>
                    )}
                  </div>
                </div>
                <div className="batch-toolbar">
                  <div className="batch-actions">
                    <span className="selection-count" aria-live="polite">
                      已选 {chosen.length} 项
                    </span>
                    <button
                      className="button small"
                      disabled={pending || !chosen.length}
                      onClick={() => safely(batch("resume"))}
                    >
                      <Play size={14} />
                      启动
                    </button>
                    <button
                      className="button small"
                      disabled={pending || !chosen.length}
                      onClick={() => safely(batch("pause"))}
                    >
                      <Pause size={14} />
                      暂停
                    </button>
                    <button
                      className="button small danger"
                      disabled={pending || !chosen.length}
                      onClick={() =>
                        setConfirm({
                          title: "删除选中的同步项目？",
                          text:
                            "将删除选中的 " +
                            chosen.length +
                            " 个项目的同步配置。本地文件和远程仓库会保留；正在同步的项目将跳过。",
                          action: () => batch("remove_task"),
                        })
                      }
                    >
                      <Trash2 size={14} />
                      删除
                    </button>
                  </div>
                  <div className="batch-actions global-actions">
                    <button
                      className="button small ghost"
                      disabled={pending || !data.tasks.length}
                      onClick={() => safely(batch("pause", true))}
                    >
                      全部暂停
                    </button>
                    <button
                      className="button small ghost"
                      disabled={pending || !data.tasks.length}
                      onClick={() => safely(batch("resume", true))}
                    >
                      全部恢复
                    </button>
                  </div>
                </div>
                {filtered.length ? (
                  <div className="table-scroll">
                    <table className="project-table">
                      <colgroup>
                        <col className="check-col" />
                        <col className="platform-col" />
                        <col />
                        <col className="branch-col" />
                        <col className="interval-col" />
                        <col className="status-col" />
                        <col className="expires-col" />
                        <col className="date-col" />
                        <col className="actions-col" />
                      </colgroup>
                      <thead>
                        <tr>
                          <th className="center">
                            <input
                              type="checkbox"
                              className="row-check"
                              aria-label="全选当前列表"
                              checked={chosen.length === filtered.length}
                              ref={(el) => {
                                if (el)
                                  el.indeterminate =
                                    chosen.length > 0 &&
                                    chosen.length < filtered.length;
                              }}
                              onChange={(e) =>
                                setChecked(
                                  e.target.checked
                                    ? filtered.map((t) => t.id)
                                    : [],
                                )
                              }
                            />
                          </th>
                          <SortHeader
                            className="center"
                            label="平台"
                            sortKey="provider"
                            sort={sort}
                            onSort={toggleSort}
                          />
                          <SortHeader
                            label="项目"
                            sortKey="name"
                            sort={sort}
                            onSort={toggleSort}
                          />
                          <SortHeader
                            label="分支"
                            sortKey="branch"
                            sort={sort}
                            onSort={toggleSort}
                          />
                          <SortHeader
                            label="同步间隔"
                            sortKey="interval"
                            sort={sort}
                            onSort={toggleSort}
                          />
                          <SortHeader
                            label="状态"
                            sortKey="status"
                            sort={sort}
                            onSort={toggleSort}
                          />
                          <SortHeader
                            label="同步期限"
                            sortKey="expires"
                            sort={sort}
                            onSort={toggleSort}
                          />
                          <SortHeader
                            label="最近同步"
                            sortKey="last_success"
                            sort={sort}
                            onSort={toggleSort}
                          />
                          <th className="center">操作</th>
                        </tr>
                      </thead>
                      <tbody>
                        {visible.map((t) => {
                          const rt = data.runtime[t.id];
                          const expired = rt?.status === "expired";
                          return (
                            <tr
                              key={t.id}
                              className={selected === t.id ? "selected" : ""}
                              onClick={() => setSelected(t.id)}
                            >
                              <td className="center">
                                <input
                                  type="checkbox"
                                  className="row-check"
                                  aria-label={"选择 " + t.name}
                                  checked={chosen.includes(t.id)}
                                  onClick={(e) => e.stopPropagation()}
                                  onChange={(e) =>
                                    setChecked((ids) =>
                                      e.target.checked
                                        ? [...ids, t.id]
                                        : ids.filter((id) => id !== t.id),
                                    )
                                  }
                                />
                              </td>
                              <td className="center">
                                <Platform provider={t.provider} />
                              </td>
                              <td>
                                <button
                                  className="project-name"
                                  onClick={() => setSelected(t.id)}
                                >
                                  {t.name}
                                </button>
                                <div
                                  className="project-path"
                                  title={t.local_dir}
                                >
                                  {t.local_dir}
                                </div>
                              </td>
                              <td>
                                <span className="branch">{t.branch}</span>
                              </td>
                              <td className="tabular">
                                {t.interval_seconds ??
                                  data.settings.interval_seconds}
                                <span className="muted">
                                  {" "}
                                  秒
                                  {t.interval_seconds === null ? " · 全局" : ""}
                                </span>
                              </td>
                              <td>
                                <span
                                  className={`status ${rt?.status || "paused"}`}
                                >
                                  <span />
                                  {labels[rt?.status || "paused"]}
                                </span>
                              </td>
                              <td
                                className="tabular date"
                                title={
                                  t.expires_at
                                    ? new Date(
                                        t.expires_at * 1000,
                                      ).toLocaleString("zh-CN") + " 到期"
                                    : "永久同步"
                                }
                              >
                                {termDate(t)}
                              </td>
                              <td className="tabular date">
                                {date(rt?.last_success)}
                              </td>
                              <td className="center">
                                <div className="row-actions">
                                  <button
                                    className="icon-button"
                                    aria-label={`立即同步 ${t.name}`}
                                    title="立即同步"
                                    disabled={
                                      pending ||
                                      rt?.status === "syncing" ||
                                      rt?.blocked ||
                                      expired
                                    }
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      safely(act("sync", { id: t.id }));
                                    }}
                                  >
                                    <RefreshCw
                                      size={15}
                                      className={
                                        rt?.status === "syncing" ? "spin" : ""
                                      }
                                    />
                                  </button>
                                  <button
                                    className="icon-button"
                                    aria-label={`${expired ? "续期" : t.enabled && !rt?.blocked ? "暂停" : "恢复"} ${t.name}`}
                                    title={
                                      expired
                                        ? "编辑项目续期"
                                        : t.enabled && !rt?.blocked
                                          ? "暂停"
                                          : "恢复"
                                    }
                                    disabled={
                                      pending ||
                                      (rt?.status === "syncing" && !t.enabled)
                                    }
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      if (expired) {
                                        setEditing(t);
                                        return;
                                      }
                                      safely(
                                        act(
                                          t.enabled && !rt?.blocked
                                            ? "pause"
                                            : "resume",
                                          { id: t.id },
                                        ),
                                      );
                                    }}
                                  >
                                    {expired ? (
                                      <Clock3 size={16} />
                                    ) : t.enabled && !rt?.blocked ? (
                                      <Pause size={16} />
                                    ) : (
                                      <Play size={16} />
                                    )}
                                  </button>
                                </div>
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                ) : (
                  <div className="empty">
                    <div className="empty-icon">
                      <ArrowLeftRight size={29} />
                    </div>
                    <h2>
                      {search ? "没有匹配的项目" : "从第一个同步项目开始"}
                    </h2>
                    <p>
                      {search
                        ? "试试其他项目名称或路径。"
                        : "连接本地仓库与 Overleaf 或 GitHub。"}
                    </p>
                    {!search && (
                      <button
                        className="button"
                        onClick={() => setEditing(null)}
                      >
                        <Plus size={16} />
                        添加项目
                      </button>
                    )}
                  </div>
                )}
              </section>
              {task ? (
                <section className="panel detail">
                  <div className="detail-title">
                    <Platform provider={task.provider} />
                    <h2>{task.name}</h2>
                    <button
                      className="icon-button"
                      aria-label="收起详情"
                      onClick={() => setSelected(null)}
                    >
                      <X size={16} />
                    </button>
                  </div>
                  <div
                    className={`detail-message ${runtime?.blocked || runtime?.status === "error" ? "danger-text" : ""}`}
                  >
                    <span className="tiny-dot" />
                    {runtime?.message || "尚未开始同步"}
                  </div>
                  <dl>
                    <dt>本地目录</dt>
                    <dd>{task.local_dir}</dd>
                    <dt>远程仓库</dt>
                    <dd>{task.remote_url}</dd>
                    <dt>同步期限</dt>
                    <dd>
                      {task.expires_at
                        ? new Date(task.expires_at * 1000).toLocaleString(
                            "zh-CN",
                          ) + " 到期"
                        : "永久"}
                    </dd>
                  </dl>
                  <div className="detail-actions">
                    <button
                      className="button small"
                      onClick={() =>
                        safely(act("open_folder", { id: task.id }))
                      }
                    >
                      <FolderOpen size={15} />
                      打开目录
                    </button>
                    <button
                      className="button small"
                      disabled={runtime?.status === "syncing"}
                      onClick={() => setEditing(task)}
                    >
                      <Pencil size={15} />
                      编辑项目
                    </button>
                    <button
                      className="button small"
                      onClick={() =>
                        void api
                          .request<string>("logs", { id: task.id })
                          .then(setLogs)
                          .catch((e) => setNotice(String(e)))
                      }
                    >
                      <FileText size={15} />
                      同步记录
                    </button>
                    <button
                      className="button small danger ghost"
                      disabled={runtime?.status === "syncing"}
                      onClick={() =>
                        setConfirm({
                          title: "删除同步项目？",
                          text: `将移除“${task.name}”的同步配置。本地文件和远程仓库会保留。`,
                          action: async () => {
                            await act("remove_task", { id: task.id });
                            setSelected(null);
                          },
                        })
                      }
                    >
                      <Trash2 size={15} />
                      删除项目
                    </button>
                  </div>
                </section>
              ) : (
                <div className="under-note">
                  <CircleHelp size={14} />
                  点击项目查看路径、同步记录和更多操作。
                </div>
              )}
            </>
          ) : (
            <SettingsPage
              data={data}
              act={act}
              onConfirm={setConfirm}
              theme={theme}
              onThemeChange={setTheme}
            />
          )}
        </div>
      </main>
      {editing !== undefined && (
        <TaskDialog
          task={editing}
          provider={filter === "all" ? "overleaf" : filter}
          data={data}
          onClose={() => setEditing(undefined)}
          onSave={async (t) => {
            await act("save_task", t);
            setEditing(undefined);
          }}
        />
      )}
      {notice && (
        <div className="toast" role="alert">
          <AlertCircle size={18} />
          <span>{notice}</span>
          <button
            className="icon-button"
            aria-label="关闭提示"
            onClick={() => setNotice("")}
          >
            <X size={16} />
          </button>
        </div>
      )}
      {confirm && (
        <Confirm
          title={confirm.title}
          text={confirm.text}
          onClose={() => setConfirm(null)}
          action={confirm.action}
        />
      )}
      {logs !== null && (
        <Modal
          title="同步记录"
          description="最近 100 条记录"
          onClose={() => setLogs(null)}
          wide
        >
          <pre className="logs">{logs || "暂无同步记录"}</pre>
        </Modal>
      )}
      {closing && (
        <Modal
          title="关闭 Git Sync"
          description="最小化后继续同步，退出后停止自动同步。"
          onClose={() => setClosing(false)}
        >
          <div className="dialog-footer">
            <button
              className="button"
              onClick={() => {
                setClosing(false);
                void getCurrentWindow().minimize();
              }}
            >
              <Minus size={16} />
              最小化
            </button>
            <button
              className="button primary"
              onClick={() =>
                void invoke("exit_app").catch((e) => setNotice(String(e)))
              }
            >
              退出程序
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}
function Confirm({
  title,
  text,
  action,
  onClose,
}: {
  title: string;
  text: string;
  action: () => Promise<void>;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  return (
    <Modal title={title} description={text} onClose={() => !busy && onClose()}>
      <ErrorText text={error} />
      <div className="dialog-footer">
        <button className="button" disabled={busy} onClick={onClose}>
          取消
        </button>
        <button
          className="button danger-solid"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await action();
              onClose();
            } catch (e) {
              setError(String(e));
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? "处理中…" : "确认删除"}
        </button>
      </div>
    </Modal>
  );
}
function TaskDialog({
  task,
  provider,
  data,
  onClose,
  onSave,
}: {
  task: Task | null;
  provider: Provider;
  data: Snapshot;
  onClose: () => void;
  onSave: (
    t: Task & {
      confirm_non_empty?: boolean;
      renew_term?: boolean;
      utc_offset_seconds?: number;
    },
  ) => Promise<void>;
}) {
  const [form, setForm] = useState<Task>(
      task ?? {
        id: "",
        name: "",
        provider,
        local_dir: "",
        remote_url: "",
        branch: "main",
        interval_seconds: null,
        enabled: true,
        token_id: null,
        term: { unit: "months", count: 1 },
        expires_at: null,
      },
    ),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const [confirmFolder, setConfirmFolder] = useState(false);
  const [renewTerm, setRenewTerm] = useState(false);
  const submission = () => ({
    ...form,
    term: task && !renewTerm ? task.term : form.term,
    renew_term: !!task && renewTerm,
    utc_offset_seconds: -new Date().getTimezoneOffset() * 60,
  });
  const patch = (value: Partial<Task>) => setForm((f) => ({ ...f, ...value }));
  async function inspect(path: string) {
    setBusy(true);
    setError("");
    try {
      const result = await api.request<
        Partial<Pick<Task, "remote_url" | "provider" | "branch">> & {
          needs_setup?: boolean;
        }
      >("inspect", { path });
      setForm((f) => ({
        ...f,
        ...(result.needs_setup ? {} : result),
        local_dir: path,
        token_id: null,
        name: f.name || path.split(/[\\/]/).filter(Boolean).pop() || "",
      }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function browse() {
    try {
      const path = api.preview
        ? "D:\\Research\\demo"
        : await open({
            directory: true,
            multiple: false,
            title: "选择本地文件夹",
          });
      if (typeof path === "string") await inspect(path);
    } catch (e) {
      setError(String(e));
    }
  }
  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const plan = await api.request<{
        needs_setup?: boolean;
        non_empty?: boolean;
      }>("inspect", { path: form.local_dir });
      if (form.provider === "github" && plan.needs_setup && plan.non_empty) {
        setConfirmFolder(true);
        return;
      }
      await onSave(submission());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  if (confirmFolder)
    return (
      <Modal
        title="使用这个非空目录？"
        description="该目录已有文件，尚未初始化为 Git 仓库。"
        onClose={() => !busy && setConfirmFolder(false)}
      >
        <div className="form-body">
          <p style={{ overflowWrap: "anywhere" }}>{form.local_dir}</p>
          <p>
            继续会关联远程仓库并保留现有文件。同名文件保留本地版本；启用同步后，本地修改会提交并上传。
          </p>
          <ErrorText text={error} />
        </div>
        <div className="dialog-footer">
          <button
            className="button"
            disabled={busy}
            onClick={() => setConfirmFolder(false)}
          >
            取消
          </button>
          <button
            className="button primary"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              setError("");
              try {
                await onSave({ ...submission(), confirm_non_empty: true });
              } catch (e) {
                setError(String(e));
              } finally {
                setBusy(false);
              }
            }}
          >
            {busy ? "正在创建…" : "继续创建"}
          </button>
        </div>
      </Modal>
    );
  return (
    <Modal
      title={task ? "编辑同步项目" : "新增同步项目"}
      description="GitHub 支持已有仓库、普通文件夹或尚未创建的目录。"
      onClose={() => !busy && onClose()}
      wide
    >
      <form onSubmit={submit}>
        <div className="form-body">
          <Segments
            value={form.provider}
            onChange={(provider) => patch({ provider, token_id: null })}
          />
          <label className="field">
            项目名称
            <input
              required
              value={form.name}
              placeholder="例如：论文源文件"
              onChange={(e) => patch({ name: e.target.value })}
            />
          </label>
          <label className="field">
            本地仓库
            <div className="input-row">
              <input
                required
                value={form.local_dir}
                placeholder="选择文件夹或输入新目录的完整路径"
                onChange={(e) => patch({ local_dir: e.target.value })}
              />
              <button
                type="button"
                className="button"
                disabled={busy}
                onClick={() => void browse()}
              >
                <FolderOpen size={16} />
                浏览
              </button>
            </div>
          </label>
          <button
            className="text-button"
            type="button"
            disabled={busy || !form.local_dir}
            onClick={() => void inspect(form.local_dir)}
          >
            读取仓库信息 <ArrowUpRight size={13} />
          </button>
          {form.provider === "github" && (
            <p style={{ fontSize: 11 }}>
              没有 Git
              仓库时会在保存时自动初始化。目录不存在则创建；已有文件保留，同名文件作为本地修改参与后续同步。
            </p>
          )}
          <label className="field">
            远程仓库地址
            <input
              required
              value={form.remote_url}
              placeholder={
                form.provider === "overleaf"
                  ? "https://git@git.overleaf.com/项目 ID"
                  : "https://github.com/用户名/仓库名"
              }
              onChange={(e) => patch({ remote_url: e.target.value })}
            />
          </label>
          <div className="field-pair">
            <label className="field">
              分支
              <input
                required
                value={form.branch}
                onChange={(e) => patch({ branch: e.target.value })}
              />
            </label>
            <label className="field">
              同步间隔（秒）
              <input
                type="number"
                min="5"
                max="86400"
                placeholder={`跟随全局 · ${data.settings.interval_seconds} 秒`}
                value={form.interval_seconds ?? ""}
                onChange={(e) =>
                  patch({
                    interval_seconds:
                      e.target.value === "" ? null : Number(e.target.value),
                  })
                }
              />
            </label>
          </div>
          <div className="term-settings">
            {task && (
              <>
                <p className="current-deadline">
                  当前到期时间：
                  {task.expires_at
                    ? new Date(task.expires_at * 1000).toLocaleString("zh-CN")
                    : "永久"}
                </p>
                <label className="field">
                  期限设置
                  <select
                    aria-label="期限设置"
                    value={renewTerm ? "reset" : "keep"}
                    onChange={(e) => {
                      const reset = e.target.value === "reset";
                      setRenewTerm(reset);
                      if (
                        task.expires_at &&
                        task.expires_at <= Date.now() / 1000
                      )
                        patch({ enabled: reset ? true : task.enabled });
                    }}
                  >
                    <option value="keep">保持当前期限</option>
                    <option value="reset">重新设置期限</option>
                  </select>
                </label>
              </>
            )}
            {(!task || renewTerm) && (
              <>
                <div className="field-pair">
                  <label className="field">
                    同步期限
                    <select
                      aria-label="同步期限"
                      value={form.term.unit}
                      onChange={(e) =>
                        patch({
                          term: {
                            unit: e.target.value as Task["term"]["unit"],
                            count: 1,
                          },
                        })
                      }
                    >
                      <option value="days">按天</option>
                      <option value="months">按月</option>
                      <option value="permanent">永久</option>
                    </select>
                  </label>
                  {form.term.unit !== "permanent" && (
                    <label className="field">
                      {form.term.unit === "days" ? "天数" : "月数"}
                      <input
                        type="number"
                        required
                        min="1"
                        max={form.term.unit === "days" ? 30 : 12}
                        value={form.term.count}
                        onChange={(e) =>
                          patch({
                            term: {
                              ...form.term,
                              count: Number(e.target.value),
                            },
                          })
                        }
                      />
                    </label>
                  )}
                </div>
                <p className="field-help">
                  {form.term.unit === "permanent"
                    ? "保存后永久同步，不设到期时间。"
                    : "保存后再同步 " +
                      form.term.count +
                      (form.term.unit === "days" ? " 天。" : " 个自然月。") +
                      "到期自动暂停，项目和文件会保留。"}
                </p>
              </>
            )}
            {task && !renewTerm && (
              <p className="field-help">保存其他修改不会改变当前到期时间。</p>
            )}
          </div>
          <label className="field">
            访问 Token
            <select
              value={form.token_id ?? ""}
              onChange={(e) => patch({ token_id: e.target.value || null })}
            >
              <option value="">使用平台默认</option>
              <option value="@system">使用系统 Git 凭据</option>
              {data.catalog.tokens
                .filter((t) => t.provider === form.provider)
                .map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.name} · {t.masked}
                  </option>
                ))}
            </select>
          </label>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={form.enabled}
              onChange={(e) => patch({ enabled: e.target.checked })}
            />
            保存后启用自动同步
          </label>
          <ErrorText text={error} />
        </div>
        <div className="dialog-footer">
          <button
            type="button"
            className="button"
            disabled={busy}
            onClick={onClose}
          >
            取消
          </button>
          <button type="submit" className="button primary" disabled={busy}>
            {busy ? "处理中…" : task ? "保存修改" : "创建同步"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
function SettingsPage({
  data,
  act,
  onConfirm,
  theme,
  onThemeChange,
}: {
  data: Snapshot;
  act: (op: string, p: unknown) => Promise<void>;
  onConfirm: (c: {
    title: string;
    text: string;
    action: () => Promise<void>;
  }) => void;
  theme: Theme;
  onThemeChange: (t: Theme) => void;
}) {
  const [settings, setSettings] = useState<Settings>(data.settings),
    [provider, setProvider] = useState<Provider>("overleaf"),
    [adding, setAdding] = useState(false),
    [error, setError] = useState(""),
    [saved, setSaved] = useState(false),
    [busy, setBusy] = useState(false);
  const tokens = data.catalog.tokens.filter((t) => t.provider === provider),
    defaultId = data.catalog.defaults[provider];
  async function action(op: string, p: unknown) {
    setBusy(true);
    setError("");
    try {
      await act(op, p);
    } catch (e) {
      setError(String(e));
      throw e;
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <header className="page-heading">
        <div>
          <div className="eyebrow">MAKE IT YOURS</div>
          <h1>全局设置</h1>
          <p>设置同步节奏，管理不同平台的访问凭据。</p>
        </div>
      </header>
      <section className="panel setting-card">
        <div className="section-heading">
          <div className="section-icon">
            <Clock3 size={20} />
          </div>
          <div>
            <h2>同步设置</h2>
            <p>项目可以使用全局设置，也可以单独指定间隔。</p>
          </div>
        </div>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            try {
              await action("save_settings", settings);
              setSaved(true);
              setTimeout(() => setSaved(false), 2200);
            } catch {}
          }}
        >
          <div className="setting-row">
            <label htmlFor="interval">
              默认同步间隔<small>检查本地和远程更改的频率</small>
            </label>
            <div className="number-unit">
              <input
                id="interval"
                type="number"
                required
                min="5"
                max="86400"
                value={settings.interval_seconds}
                onChange={(e) =>
                  setSettings((s) => ({
                    ...s,
                    interval_seconds: Number(e.target.value),
                  }))
                }
              />
              <span>秒</span>
            </div>
          </div>
          <div className="setting-row">
            <label htmlFor="settle">
              文件保存等待<small>等待连续写入结束后再提交</small>
            </label>
            <div className="number-unit">
              <input
                id="settle"
                type="number"
                required
                min="1"
                max="60"
                value={settings.settle_seconds}
                onChange={(e) =>
                  setSettings((s) => ({
                    ...s,
                    settle_seconds: Number(e.target.value),
                  }))
                }
              />
              <span>秒</span>
            </div>
          </div>
          <div className="setting-footer">
            {saved && (
              <span className="saved">
                <Check size={15} />
                设置已保存
              </span>
            )}
            <button className="button primary small" disabled={busy}>
              保存设置
            </button>
          </div>
        </form>
      </section>
      <section className="panel setting-card">
        <div className="section-heading">
          <div className="section-icon">
            <Palette size={20} />
          </div>
          <div>
            <h2>外观</h2>
            <p>选择应用界面的明暗主题，跟随系统会随操作系统自动切换。</p>
          </div>
        </div>
        <div className="theme-row">
          {(
            [
              { value: "system", label: "跟随系统", icon: Monitor },
              { value: "light", label: "浅色", icon: Sun },
              { value: "dark", label: "深色", icon: Moon },
            ] as const
          ).map(({ value, label, icon: Icon }) => (
            <button
              key={value}
              type="button"
              className={`theme-option${theme === value ? " active" : ""}`}
              aria-pressed={theme === value}
              onClick={() => onThemeChange(value)}
            >
              <Icon size={16} />
              <span>{label}</span>
              {theme === value && <Check size={14} />}
            </button>
          ))}
        </div>
      </section>
      <section className="panel setting-card token-card">
        <div className="section-heading">
          <div className="section-icon">
            <KeyRound size={20} />
          </div>
          <div>
            <h2>访问 Token</h2>
            <p>按平台管理 Token，项目可选择默认或指定凭据。</p>
          </div>
        </div>
        <div className="token-toolbar">
          <Segments value={provider} onChange={setProvider} />
          <button className="button small" onClick={() => setAdding(true)}>
            <Plus size={15} />
            新增 Token
          </button>
        </div>
        <div className="default-note">
          <span>
            平台默认：
            <strong>
              {tokens.find((t) => t.id === defaultId)?.name || "系统 Git 凭据"}
            </strong>
          </span>
          {defaultId && (
            <button
              className="text-button"
              disabled={busy}
              onClick={() =>
                void action("default_token", { provider, id: null }).catch(
                  () => {},
                )
              }
            >
              改用系统凭据
            </button>
          )}
        </div>
        {tokens.length ? (
          <div className="table-scroll">
            <table className="token-table">
              <colgroup>
                <col style={{ width: "30%" }} />
                <col style={{ width: "32%" }} />
                <col style={{ width: "16%" }} />
                <col style={{ width: "22%" }} />
              </colgroup>
              <thead>
                <tr>
                  <th>名称</th>
                  <th>Token</th>
                  <th>默认</th>
                  <th className="right">操作</th>
                </tr>
              </thead>
              <tbody>
                {tokens.map((t) => (
                  <tr key={t.id}>
                    <td>
                      <span className="token-name">
                        <KeyRound size={15} />
                        {t.name}
                      </span>
                    </td>
                    <td className="token-mask">{t.masked}</td>
                    <td>
                      {t.id === defaultId ? (
                        <span className="default-badge">
                          <Check size={13} />
                          默认
                        </span>
                      ) : (
                        <span className="muted">—</span>
                      )}
                    </td>
                    <td>
                      <div className="token-actions">
                        {t.id !== defaultId && (
                          <button
                            className="text-button"
                            disabled={busy}
                            onClick={() =>
                              void action("default_token", {
                                provider,
                                id: t.id,
                              }).catch(() => {})
                            }
                          >
                            设为默认
                          </button>
                        )}
                        <button
                          className="icon-button danger"
                          aria-label={`删除 Token ${t.name}`}
                          title="删除 Token"
                          onClick={() =>
                            onConfirm({
                              title: "删除 Token？",
                              text: `将移除“${t.name}”。如果它被项目单独指定，需要先更换项目的 Token。`,
                              action: () =>
                                action("remove_token", { id: t.id }),
                            })
                          }
                        >
                          <Trash2 size={15} />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="token-empty">
            <KeyRound size={24} />
            <div>
              还没有添加 {names[provider]} Token
              <p>当前使用系统 Git 凭据，也可以添加专用 Token。</p>
            </div>
          </div>
        )}
        <div className="panel-footnote">
          Token 的新增、删除和默认选择即时生效。
        </div>
      </section>
      <ErrorText text={error} />
      {adding && (
        <TokenDialog
          provider={provider}
          first={!defaultId}
          onClose={() => setAdding(false)}
          onSave={async (p) => {
            await action("add_token", p);
            setAdding(false);
          }}
        />
      )}
    </>
  );
}
function TokenDialog({
  provider,
  first,
  onClose,
  onSave,
}: {
  provider: Provider;
  first: boolean;
  onClose: () => void;
  onSave: (p: unknown) => Promise<void>;
}) {
  const [name, setName] = useState(""),
    [token, setToken] = useState(""),
    [makeDefault, setDefault] = useState(first),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  return (
    <Modal
      title={`新增 ${names[provider]} Token`}
      description="为 Token 起一个便于识别的名称。"
      onClose={() => !busy && onClose()}
    >
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          try {
            await onSave({ provider, name, token, make_default: makeDefault });
            setToken("");
          } catch (e) {
            setError(String(e));
          } finally {
            setBusy(false);
          }
        }}
      >
        <div className="form-body">
          <label className="field">
            名称
            <input
              required
              autoFocus
              placeholder="例如：个人账户"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          <label className="field">
            Token
            <input
              required
              type="password"
              autoComplete="off"
              spellCheck={false}
              placeholder="粘贴访问 Token"
              value={token}
              onChange={(e) => setToken(e.target.value)}
            />
          </label>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={makeDefault}
              onChange={(e) => setDefault(e.target.checked)}
            />
            设为 {names[provider]} 默认 Token
          </label>
          <ErrorText text={error} />
        </div>
        <div className="dialog-footer">
          <button
            type="button"
            className="button"
            disabled={busy}
            onClick={onClose}
          >
            取消
          </button>
          <button className="button primary" disabled={busy}>
            {busy ? "保存中…" : "添加 Token"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
