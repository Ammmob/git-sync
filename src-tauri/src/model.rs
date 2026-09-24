use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io::Write, path::Path};

pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub interval_seconds: u64,
    pub settle_seconds: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            interval_seconds: 10,
            settle_seconds: 2,
        }
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncTerm {
    pub unit: String,
    pub count: u32,
}
impl Default for SyncTerm {
    fn default() -> Self {
        Self {
            unit: "permanent".into(),
            count: 1,
        }
    }
}
impl SyncTerm {
    pub fn month() -> Self {
        Self {
            unit: "months".into(),
            count: 1,
        }
    }
    pub fn validate(&self) -> Result<()> {
        match self.unit.as_str() {
            "days" if (1..=30).contains(&self.count) => Ok(()),
            "months" if (1..=12).contains(&self.count) => Ok(()),
            "permanent" if self.count == 1 => Ok(()),
            _ => Err("期限应为 1–30 天、1–12 个月或永久".into()),
        }
    }
    pub fn deadline(&self, start: u64, offset_seconds: i32) -> Result<Option<u64>> {
        self.validate()?;
        let offset = time::UtcOffset::from_whole_seconds(offset_seconds).map_err(|_| "时区无效")?;
        if self.unit == "permanent" {
            return Ok(None);
        }
        if self.unit == "days" {
            return start
                .checked_add(u64::from(self.count) * 86400)
                .map(Some)
                .ok_or("日期超出范围".into());
        }
        let dt =
            time::OffsetDateTime::from_unix_timestamp(start.try_into().map_err(|_| "日期无效")?)
                .map_err(|_| "日期无效")?
                .to_offset(offset);
        let month_index = dt.year() * 12 + i32::from(u8::from(dt.month())) - 1 + self.count as i32;
        let year = month_index / 12;
        let month = time::Month::try_from((month_index % 12 + 1) as u8).map_err(|_| "月份无效")?;
        let mut day = dt.day();
        let date = loop {
            if let Ok(date) = time::Date::from_calendar_date(year, month, day) {
                break date;
            }
            if day == 1 {
                return Err("日期超出范围".into());
            }
            day -= 1;
        };
        Ok(Some(
            dt.replace_date(date)
                .unix_timestamp()
                .try_into()
                .map_err(|_| "日期无效")?,
        ))
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub local_dir: String,
    pub remote_url: String,
    pub branch: String,
    pub interval_seconds: Option<u64>,
    pub enabled: bool,
    pub token_id: Option<String>,
    #[serde(default)]
    pub term: SyncTerm,
    #[serde(default)]
    pub expires_at: Option<u64>,
}
impl Task {
    pub fn expired(&self, timestamp: u64) -> bool {
        self.expires_at.is_some_and(|end| end <= timestamp)
    }
    pub fn update_term(
        &mut self,
        old: Option<&Task>,
        renew: bool,
        timestamp: u64,
        offset: i32,
    ) -> Result<()> {
        self.term.validate()?;
        self.expires_at = if renew || old.is_none_or(|t| t.term != self.term) {
            self.term.deadline(timestamp, offset)?
        } else {
            old.and_then(|t| t.expires_at)
        };
        if self.expired(timestamp) {
            self.enabled = false;
        }
        Ok(())
    }
}
pub fn expire_tasks(store: &mut Store, timestamp: u64) -> bool {
    let mut changed = false;
    for task in &mut store.tasks {
        if task.expired(timestamp) {
            let rt = store.runtime.entry(task.id.clone()).or_default();
            if task.enabled || (!rt.running && rt.status != "expired") {
                changed = true;
            }
            task.enabled = false;
            if !rt.running {
                rt.status = "expired".into();
                rt.message = "同步期限已到，请编辑项目续期或改为永久".into();
            }
        }
    }
    changed
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Runtime {
    pub status: String,
    pub message: String,
    pub last_success: Option<u64>,
    #[serde(default)]
    pub blocked: bool,
    #[serde(skip)]
    pub running: bool,
    #[serde(skip)]
    pub next_run: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Token {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub masked: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub tokens: Vec<Token>,
    pub defaults: HashMap<String, Option<String>>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Store {
    pub settings: Settings,
    pub tasks: Vec<Task>,
    pub catalog: Catalog,
    pub runtime: HashMap<String, Runtime>,
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn read_store(dir: &Path) -> Result<Store> {
    let path = dir.join("config.json");
    if !path.exists() {
        return Ok(Store::default());
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let mut store: Store = serde_json::from_slice(&bytes)
        .map_err(|_| "配置文件无法读取，请保留文件并检查格式。".to_string())?;
    for (id, rt) in store.runtime.iter_mut() {
        rt.status = if rt.blocked {
            "blocked"
        } else if store.tasks.iter().any(|t| &t.id == id && t.enabled) {
            "waiting"
        } else {
            "paused"
        }
        .into();
    }
    if expire_tasks(&mut store, now()) {
        persist(dir, &store)?;
    }
    Ok(store)
}
pub fn persist(dir: &Path, store: &Store) -> Result<()> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut temp, store).map_err(|e| e.to_string())?;
    temp.flush().map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(dir.join("config.json"))
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn provider(value: &str) -> Result<()> {
    if value == "github" || value == "overleaf" {
        Ok(())
    } else {
        Err("不支持的平台".into())
    }
}
pub fn normalize_url(value: &str, expected: &str) -> Result<String> {
    provider(expected)?;
    let u = url::Url::parse(value.trim()).map_err(|_| "请输入有效的 HTTPS 仓库地址".to_string())?;
    if u.scheme() != "https"
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.port().is_some()
    {
        return Err("请使用 HTTPS 仓库地址，不要在地址中包含 Token 或额外参数".into());
    }
    let parts: Vec<_> = u.path().trim_matches('/').split('/').collect();
    match (u.host_str().unwrap_or(""), expected) {
        ("github.com", "github")
            if u.username().is_empty()
                && parts.len() == 2
                && parts.iter().all(|s| !s.is_empty()) =>
        {
            let repo = parts[1].trim_end_matches(".git");
            if repo.is_empty() {
                return Err("仓库名称不能为空".into());
            }
            Ok(format!("https://github.com/{}/{}.git", parts[0], repo))
        }
        ("git.overleaf.com", "overleaf")
            if (u.username().is_empty() || u.username() == "git")
                && parts.len() == 1
                && !parts[0].is_empty() =>
        {
            Ok(format!("https://git@git.overleaf.com/{}", parts[0]))
        }
        ("overleaf.com" | "www.overleaf.com", "overleaf")
            if u.username().is_empty()
                && parts.len() == 2
                && parts[0] == "project"
                && !parts[1].is_empty() =>
        {
            Ok(format!("https://git@git.overleaf.com/{}", parts[1]))
        }
        _ => Err("仓库地址与所选平台不匹配".into()),
    }
}
pub fn mask(value: &str) -> String {
    let c: Vec<char> = value.chars().collect();
    if c.len() < 10 {
        return "*".repeat(c.len());
    }
    format!(
        "{}********{}",
        c[..4].iter().collect::<String>(),
        c[c.len() - 4..].iter().collect::<String>()
    )
}
