use crate::{credentials, engine, model::*, setup};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

pub struct Service {
    pub dir: PathBuf,
    pub store: Mutex<Store>,
    pub stop: AtomicBool,
    pub preparing: Mutex<()>,
}
impl Service {
    pub fn new(dir: PathBuf) -> Result<Arc<Self>> {
        credentials::init(&dir)?;
        let store = read_store(&dir)?;
        Ok(Arc::new(Self {
            dir,
            store: Mutex::new(store),
            stop: AtomicBool::new(false),
            preparing: Mutex::new(()),
        }))
    }
    pub fn snapshot(&self) -> Store {
        self.store.lock().unwrap().clone()
    }
    pub fn active(&self) -> bool {
        if self.preparing.try_lock().is_err() {
            return true;
        }
        self.store
            .lock()
            .unwrap()
            .runtime
            .values()
            .any(|r| r.running)
    }
    fn update<T>(&self, f: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
        let mut guard = self.store.lock().unwrap();
        let mut next = guard.clone();
        let value = f(&mut next)?;
        persist(&self.dir, &next)?;
        *guard = next;
        Ok(value)
    }
    pub fn start(self: &Arc<Self>) {
        let service = self.clone();
        thread::spawn(move || {
            while !service.stop.load(Ordering::SeqCst) {
                let _ = service.expire_due();
                let ids = {
                    let s = service.store.lock().unwrap();
                    s.tasks
                        .iter()
                        .filter(|t| {
                            t.enabled
                                && !t.expired(now())
                                && s.runtime
                                    .get(&t.id)
                                    .is_none_or(|r| !r.running && !r.blocked && r.next_run <= now())
                        })
                        .map(|t| t.id.clone())
                        .collect::<Vec<_>>()
                };
                for id in ids {
                    let _ = service.launch(&id, false);
                }
                thread::sleep(Duration::from_millis(500));
            }
        });
    }
    pub fn expire_due(&self) -> Result<()> {
        let mut guard = self.store.lock().unwrap();
        let mut next = guard.clone();
        if expire_tasks(&mut next, now()) {
            persist(&self.dir, &next)?;
            *guard = next;
        }
        Ok(())
    }
    pub fn launch(self: &Arc<Self>, id: &str, manual: bool) -> Result<()> {
        let _preparing = self
            .preparing
            .try_lock()
            .map_err(|_| "正在创建项目，请稍后再试".to_string())?;
        let (task, catalog, settle) = {
            let mut s = self.store.lock().unwrap();
            let task = s
                .tasks
                .iter()
                .find(|t| t.id == id)
                .cloned()
                .ok_or("项目不存在")?;
            if task.expired(now()) {
                return Err("同步期限已到，请编辑项目续期或改为永久".into());
            }
            if !manual && !task.enabled {
                return Err("项目已暂停".into());
            }
            if s.runtime.values().filter(|r| r.running).count() >= 4 {
                return Err("当前已有 4 个同步任务在运行，请稍后再试".into());
            }
            let rt = s.runtime.entry(id.into()).or_default();
            if rt.running {
                return Err("该项目正在同步".into());
            }
            if rt.blocked {
                return Err("该任务已因冲突或仓库状态暂停，请处理后点击恢复".into());
            }
            rt.running = true;
            rt.status = "syncing".into();
            rt.message = "正在检查并同步本地与远程更改".into();
            (task, s.catalog.clone(), s.settings.settle_seconds)
        };
        let service = self.clone();
        thread::spawn(move || {
            let outcome = std::panic::catch_unwind(|| {
                engine::cycle(&task, &catalog, &service.dir, settle, false)
            })
            .unwrap_or_else(|_| {
                Err(engine::Failure {
                    message: "同步进程发生异常，请重试".into(),
                    blocked: true,
                })
            });
            let mut s = service.store.lock().unwrap();
            let interval = task.interval_seconds.unwrap_or(s.settings.interval_seconds);
            let enabled = s.tasks.iter().any(|t| t.id == task.id && t.enabled);
            let rt = s.runtime.entry(task.id.clone()).or_default();
            rt.running = false;
            rt.next_run = now() + interval;
            match outcome {
                Ok(value) => {
                    rt.blocked = false;
                    rt.message = match value {
                        "busy" => "文件仍在保存，稍后重试",
                        "unchanged" => "本地与远程已一致",
                        _ => "本地与远程更改已同步",
                    }
                    .into();
                    if value != "busy" {
                        rt.last_success = Some(now());
                    }
                    rt.status = if !enabled {
                        "paused"
                    } else if value == "busy" {
                        "waiting"
                    } else {
                        "synced"
                    }
                    .into();
                }
                Err(e) => {
                    rt.blocked = e.blocked;
                    rt.message = e.message;
                    rt.status = if e.blocked {
                        "blocked"
                    } else if enabled {
                        "error"
                    } else {
                        "paused"
                    }
                    .into();
                }
            }
            expire_tasks(&mut s, now());
            let message = s.runtime[&task.id].message.clone();
            if persist(&service.dir, &s).is_err() {
                let rt = s.runtime.get_mut(&task.id).unwrap();
                rt.blocked = true;
                rt.status = "blocked".into();
                rt.message = "无法保存运行状态，请检查磁盘空间和目录权限".into();
            }
            drop(s);
            service.log(&task.id, &message);
        });
        Ok(())
    }
    fn log(&self, id: &str, message: &str) {
        let folder = self.dir.join("logs");
        let _ = fs::create_dir_all(&folder);
        let path = folder.join(format!("{id}.log"));
        if fs::metadata(&path).is_ok_and(|m| m.len() > 1_000_000) {
            let _ = fs::remove_file(path.with_extension("previous.log"));
            let _ = fs::rename(&path, path.with_extension("previous.log"));
        }
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{} {}", now(), message);
        }
    }
    pub fn request(self: &Arc<Self>, op: &str, p: Value) -> Result<Value> {
        let field = |key: &str| -> Result<String> {
            p.get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("缺少参数：{key}"))
        };
        match op {
            "inspect" => match setup::probe(&field("path")?, &self.dir)? {
                Some((url, provider, branch)) => {
                    Ok(json!({"remote_url":url,"provider":provider,"branch":branch}))
                }
                None => {
                    Ok(json!({"needs_setup":true,"non_empty":setup::non_empty(&field("path")?)?}))
                }
            },
            "save_task" => {
                let _preparing = self
                    .preparing
                    .try_lock()
                    .map_err(|_| "正在创建其他项目，请稍后再试".to_string())?;
                let confirm_non_empty = p
                    .get("confirm_non_empty")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let renew = p
                    .get("renew_term")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let has_term = p.get("term").is_some();
                let offset = p
                    .get("utc_offset_seconds")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let offset: i32 = offset.try_into().map_err(|_| "时区无效")?;
                let mut task: Task = serde_json::from_value(p).map_err(|_| "项目配置不完整")?;
                task.name = task.name.trim().into();
                task.branch = task.branch.trim().into();
                if task.name.is_empty() || task.branch.is_empty() {
                    return Err("请填写项目名称和分支".into());
                }
                if task.interval_seconds.is_some_and(|n| n < 5 || n > 86400) {
                    return Err("同步间隔应为 5–86400 秒".into());
                }
                task.remote_url = normalize_url(&task.remote_url, &task.provider)?;
                let current = self.snapshot();
                let old = current.tasks.iter().find(|t| t.id == task.id);
                if !has_term {
                    task.term = old.map(|t| t.term.clone()).unwrap_or_else(SyncTerm::month);
                }
                task.term.validate()?;
                if current.runtime.get(&task.id).is_some_and(|r| r.running) {
                    return Err("请等待当前同步结束后修改".into());
                }
                let candidate = PathBuf::from(task.local_dir.trim());
                let normalized = candidate
                    .canonicalize()
                    .unwrap_or(candidate)
                    .to_string_lossy()
                    .trim_start_matches(r"\\?\")
                    .to_lowercase();
                if current.tasks.iter().any(|t| {
                    t.id != task.id
                        && t.local_dir.trim_start_matches(r"\\?\").to_lowercase() == normalized
                }) {
                    return Err("该目录已经添加了同步任务".into());
                }
                credentials::selected(&self.dir, &task, &current.catalog)?;
                if setup::probe(&task.local_dir, &self.dir)?.is_none() {
                    setup::prepare(&task, &current.catalog, &self.dir, false, confirm_non_empty)?;
                }
                let path = PathBuf::from(&task.local_dir)
                    .canonicalize()
                    .map_err(|_| "本地目录不存在")?;
                task.local_dir = path.to_string_lossy().trim_start_matches(r"\\?\").into();
                let (url, provider, branch) = engine::inspect(&task.local_dir, &self.dir)?;
                if url != task.remote_url || provider != task.provider || branch != task.branch {
                    return Err("配置与本地仓库的 origin 或当前分支不一致".into());
                }
                task.update_term(old, renew, now(), offset)?;
                if task.id.is_empty() {
                    task.id = uuid::Uuid::new_v4().simple().to_string();
                }
                let id = task.id.clone();
                self.update(|s| {
                    if s.runtime.get(&id).is_some_and(|r| r.running) {
                        return Err("请等待当前同步结束后修改".into());
                    }
                    if s.tasks.iter().any(|t| {
                        t.id != id && t.local_dir.to_lowercase() == task.local_dir.to_lowercase()
                    }) {
                        return Err("该目录已经添加了同步任务".into());
                    }
                    credentials::selected(&self.dir, &task, &s.catalog)?;
                    let rt = s.runtime.entry(id.clone()).or_default();
                    rt.status = if task.expired(now()) {
                        "expired"
                    } else if rt.blocked {
                        "blocked"
                    } else if task.enabled {
                        "waiting"
                    } else {
                        "paused"
                    }
                    .into();
                    rt.message = if task.expired(now()) {
                        "同步期限已到，请编辑项目续期或改为永久"
                    } else if rt.blocked {
                        "请处理仓库状态后恢复同步"
                    } else if task.enabled {
                        "等待同步"
                    } else {
                        "已暂停自动同步"
                    }
                    .into();
                    rt.next_run = 0;
                    if let Some(old) = s.tasks.iter_mut().find(|t| t.id == id) {
                        *old = task;
                    } else {
                        s.tasks.push(task);
                    }
                    Ok(())
                })?;
                Ok(json!(id))
            }
            "remove_task" | "pause" | "resume" | "batch_tasks" => {
                let _preparing = self
                    .preparing
                    .try_lock()
                    .map_err(|_| "正在保存项目，请稍后再试")?;
                let action = if op == "batch_tasks" {
                    field("action")?
                } else {
                    op.into()
                };
                if !["pause", "resume", "remove_task"].contains(&action.as_str()) {
                    return Err("批量操作无效".into());
                }
                let all =
                    op == "batch_tasks" && p.get("all").and_then(Value::as_bool) == Some(true);
                let ids: Vec<String> = if all {
                    vec![]
                } else if op == "batch_tasks" {
                    serde_json::from_value(p.get("ids").cloned().ok_or("请选择项目")?)
                        .map_err(|_| "项目列表无效")?
                } else {
                    vec![field("id")?]
                };
                self.update(|s| {
                    let ids = if all {
                        s.tasks.iter().map(|t| t.id.clone()).collect()
                    } else {
                        ids
                    };
                    let mut seen = std::collections::HashSet::new();
                    let mut skipped = Vec::new();
                    let mut processed = 0;
                    for id in ids {
                        if !seen.insert(id.clone()) {
                            continue;
                        }
                        let Some(task) = s.tasks.iter_mut().find(|t| t.id == id) else {
                            skipped.push(json!({"id":id,"name":id,"reason":"项目不存在"}));
                            continue;
                        };
                        let rt = s.runtime.entry(id.clone()).or_default();
                        let reason = if action == "remove_task" && rt.running {
                            Some("正在同步，请暂停并等待本轮结束后删除")
                        } else if action == "resume" && task.expired(now()) {
                            Some("期限已到，请先编辑项目续期")
                        } else {
                            None
                        };
                        if let Some(reason) = reason {
                            skipped.push(json!({"id":id,"name":task.name,"reason":reason}));
                            continue;
                        }
                        match action.as_str() {
                            "pause" => {
                                task.enabled = false;
                                if !rt.running && !rt.blocked {
                                    rt.status = "paused".into();
                                    rt.message = "已暂停自动同步".into();
                                }
                            }
                            "resume" => {
                                task.enabled = true;
                                if !rt.running {
                                    rt.blocked = false;
                                    rt.next_run = 0;
                                    rt.status = "waiting".into();
                                    rt.message = "等待同步".into();
                                }
                            }
                            _ => {
                                s.tasks.retain(|t| t.id != id);
                                s.runtime.remove(&id);
                            }
                        }
                        processed += 1;
                    }
                    expire_tasks(s, now());
                    if op != "batch_tasks" && !skipped.is_empty() {
                        return Err(skipped[0]["reason"].as_str().unwrap().into());
                    }
                    Ok(json!({"processed":processed,"skipped":skipped}))
                })
            }
            "sync" => {
                self.launch(&field("id")?, true)?;
                Ok(Value::Null)
            }
            "save_settings" => {
                let settings: Settings = serde_json::from_value(p).map_err(|_| "设置格式无效")?;
                if !(5..=86400).contains(&settings.interval_seconds)
                    || !(1..=60).contains(&settings.settle_seconds)
                {
                    return Err("同步间隔应为 5–86400 秒，保存等待应为 1–60 秒".into());
                }
                self.update(|s| {
                    s.settings = settings;
                    for rt in s.runtime.values_mut() {
                        rt.next_run = now() + s.settings.interval_seconds;
                    }
                    Ok(())
                })?;
                Ok(Value::Null)
            }
            "add_token" => {
                let provider = field("provider")?;
                crate::model::provider(&provider)?;
                let token = field("token")?;
                let token = token.trim();
                if token.is_empty() || !token.is_ascii() {
                    return Err("请输入有效的 Token".into());
                }
                let name = field("name")?.trim().to_string();
                if name.is_empty() {
                    return Err("请填写 Token 名称".into());
                }
                let id = uuid::Uuid::new_v4().simple().to_string();
                credentials::save(&self.dir, &id, token)?;
                let entry = Token {
                    id: id.clone(),
                    name,
                    provider: provider.clone(),
                    masked: mask(token),
                };
                let result = self.update(|s| {
                    s.catalog.tokens.push(entry);
                    if p.get("make_default")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        s.catalog.defaults.insert(provider, Some(id.clone()));
                    }
                    Ok(())
                });
                if result.is_err() {
                    let _ = fs::remove_file(credentials::token_path(&self.dir, &id)?);
                }
                result?;
                Ok(Value::Null)
            }
            "default_token" => {
                let provider = field("provider")?;
                crate::model::provider(&provider)?;
                let id = p.get("id").and_then(Value::as_str).map(str::to_string);
                self.update(|s| {
                    if id.as_ref().is_some_and(|id| {
                        !s.catalog
                            .tokens
                            .iter()
                            .any(|t| &t.id == id && t.provider == provider)
                    }) {
                        return Err("Token 不属于此平台".into());
                    }
                    s.catalog.defaults.insert(provider, id);
                    Ok(())
                })?;
                Ok(Value::Null)
            }
            "remove_token" => {
                let _preparing = self
                    .preparing
                    .try_lock()
                    .map_err(|_| "正在创建项目，请稍后删除 Token".to_string())?;
                let id = field("id")?;
                let path = credentials::token_path(&self.dir, &id)?;
                self.update(|s| {
                    if s.runtime.values().any(|r| r.running) {
                        return Err("请等待当前同步结束后删除 Token".into());
                    }
                    let names: Vec<_> = s
                        .tasks
                        .iter()
                        .filter(|t| t.token_id.as_deref() == Some(&id))
                        .map(|t| t.name.clone())
                        .collect();
                    if !names.is_empty() {
                        return Err(format!("请先更换这些项目的 Token：{}", names.join("、")));
                    }
                    s.catalog.tokens.retain(|t| t.id != id);
                    for default in s.catalog.defaults.values_mut() {
                        if default.as_deref() == Some(&id) {
                            *default = None;
                        }
                    }
                    Ok(())
                })?;
                if path.exists() {
                    fs::remove_file(path).map_err(|_| "Token 已从列表移除，但凭据文件删除失败")?;
                }
                Ok(Value::Null)
            }
            "logs" => {
                let id = field("id")?;
                if !self.snapshot().tasks.iter().any(|t| t.id == id) {
                    return Err("项目不存在".into());
                }
                let text = fs::read_to_string(self.dir.join("logs").join(format!("{id}.log")))
                    .unwrap_or_default();
                Ok(json!(text
                    .lines()
                    .rev()
                    .take(100)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n")))
            }
            "open_folder" => {
                let id = field("id")?;
                let s = self.snapshot();
                let task = s.tasks.iter().find(|t| t.id == id).ok_or("项目不存在")?;
                credentials::hidden(&mut std::process::Command::new("explorer.exe"))
                    .arg(&task.local_dir)
                    .spawn()
                    .map_err(|_| "无法打开文件夹")?;
                Ok(Value::Null)
            }
            _ => Err("不支持的操作".into()),
        }
    }
}
