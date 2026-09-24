use crate::{
    credentials,
    model::{self, Catalog, Result, Task},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use wait_timeout::ChildExt;

#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub blocked: bool,
}
impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self {
            message,
            blocked: false,
        }
    }
}
type SyncResult<T> = std::result::Result<T, Failure>;
fn blocked(message: impl Into<String>) -> Failure {
    Failure {
        message: message.into(),
        blocked: true,
    }
}

pub struct Git {
    repo: PathBuf,
    credential: Option<PathBuf>,
    dir: PathBuf,
    username: &'static str,
}
impl Git {
    pub fn new(repo: &Path, dir: &Path, credential: Option<PathBuf>, provider: &str) -> Self {
        Self {
            repo: repo.into(),
            credential,
            dir: dir.into(),
            username: if provider == "overleaf" {
                "git"
            } else {
                "x-access-token"
            },
        }
    }
    pub fn call(&self, args: &[&str]) -> Result<(i32, Vec<u8>)> {
        let mut cmd = Command::new("git");
        credentials::hidden(&mut cmd)
            .arg("-c")
            .arg("core.quotePath=false")
            .arg("-c")
            .arg("core.editor=true");
        if let Some(path) = &self.credential {
            cmd.args(["-c", "credential.helper="])
                .env("GIT_SYNC_CREDENTIAL", path)
                .env("GIT_SYNC_USERNAME", self.username)
                .env("GIT_ASKPASS", self.dir.join("helpers/askpass.cmd"));
        }
        cmd.args(args)
            .current_dir(&self.repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("GIT_MERGE_AUTOEDIT", "no")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|_| "无法运行 Git，请检查是否已安装 Git 及本地目录是否存在".to_string())?;
        let mut stdout = child.stdout.take().unwrap();
        let reader = thread::spawn(move || {
            let mut b = Vec::new();
            let _ = stdout.read_to_end(&mut b);
            b
        });
        let status = child
            .wait_timeout(Duration::from_secs(120))
            .map_err(|e| e.to_string())?;
        match status {
            Some(s) => Ok((s.code().unwrap_or(-1), reader.join().unwrap_or_default())),
            None => {
                #[cfg(windows)]
                {
                    let _ = credentials::hidden(&mut Command::new("taskkill.exe"))
                        .args(["/PID", &child.id().to_string(), "/T", "/F"])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
                let _ = child.kill();
                let _ = child.wait();
                Err("Git 操作超时，将按同步间隔重试".into())
            }
        }
    }
    pub fn bytes(&self, args: &[&str]) -> Result<Vec<u8>> {
        let (code, out) = self.call(args)?;
        if code != 0 {
            return Err(format!(
                "Git {} 失败（{}），请检查网络、仓库权限和 Token",
                args[0], code
            ));
        }
        Ok(out)
    }
    pub fn text(&self, args: &[&str]) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.bytes(args)?)
            .trim()
            .to_string())
    }
    fn fingerprint(&self) -> Result<Vec<u8>> {
        let mut digest = Sha256::new();
        digest.update(self.bytes(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?);
        let files = self.bytes(&[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])?;
        let mut paths: Vec<_> = files.split(|b| *b == 0).filter(|s| !s.is_empty()).collect();
        paths.sort();
        paths.dedup();
        for raw in paths {
            digest.update(raw);
            let name = String::from_utf8_lossy(raw);
            match fs::symlink_metadata(self.repo.join(name.as_ref())) {
                Ok(st) => {
                    digest.update(st.len().to_le_bytes());
                    digest.update(format!("{:?}", st.modified()).as_bytes());
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => digest.update(b"deleted"),
                Err(_) => return Err("无法读取工作目录中的文件状态".into()),
            }
        }
        Ok(digest.finalize().to_vec())
    }
}
pub fn inspect(path: &str, dir: &Path) -> Result<(String, String, String)> {
    let repo = Path::new(path)
        .canonicalize()
        .map_err(|_| "本地目录不存在".to_string())?;
    let git = Git::new(&repo, dir, None, "github");
    let root = PathBuf::from(git.text(&["rev-parse", "--show-toplevel"])?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if root != repo {
        return Err("请选择 Git 仓库的根目录".into());
    }
    let url = git.text(&["remote", "get-url", "origin"])?;
    let provider = if model::normalize_url(&url, "overleaf").is_ok() {
        "overleaf"
    } else {
        "github"
    };
    Ok((
        model::normalize_url(&url, provider)?,
        provider.into(),
        git.text(&["symbolic-ref", "--short", "HEAD"])?,
    ))
}
pub fn validate(git: &Git, task: &Task, offline: bool) -> SyncResult<()> {
    let root = PathBuf::from(git.text(&["rev-parse", "--show-toplevel"])?)
        .canonicalize()
        .map_err(|_| blocked("仓库根目录不可用"))?;
    if root != git.repo {
        return Err(blocked("本地目录必须是仓库根目录"));
    }
    let mut actual = git.text(&["remote", "get-url", "origin"])?;
    if !offline {
        actual = model::normalize_url(&actual, &task.provider)?;
    }
    if actual != task.remote_url {
        return Err(blocked("origin 与任务中的远程地址不一致，请修改任务配置"));
    }
    if git.text(&["symbolic-ref", "--short", "HEAD"])? != task.branch {
        return Err(blocked("当前分支与任务设置不同，请切换回配置的分支后恢复"));
    }
    if git.call(&["check-ref-format", "--branch", &task.branch])?.0 != 0 {
        return Err(blocked("分支名称无效"));
    }
    let gitdir = PathBuf::from(git.text(&["rev-parse", "--absolute-git-dir"])?);
    for marker in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
        "index.lock",
    ] {
        if gitdir.join(marker).exists() {
            return Err(blocked("仓库有未完成的 Git 操作，请处理后恢复同步"));
        }
    }
    if !git.text(&["ls-files", "-u"])?.is_empty() {
        return Err(blocked("存在未解决的冲突，请处理后恢复同步"));
    }
    if !git
        .text(&["ls-files", "-ci", "--exclude-standard"])?
        .is_empty()
    {
        return Err(blocked("有已跟踪文件匹配 .gitignore，请先检查这些文件"));
    }
    Ok(())
}
pub fn cycle(
    task: &Task,
    catalog: &Catalog,
    dir: &Path,
    settle: u64,
    offline: bool,
) -> SyncResult<&'static str> {
    let repo = Path::new(&task.local_dir)
        .canonicalize()
        .map_err(|_| blocked("本地仓库目录不存在"))?;
    let cred = if offline {
        None
    } else {
        credentials::selected(dir, task, catalog)?
    };
    let git = Git::new(&repo, dir, cred, &task.provider);
    let gitdir = PathBuf::from(git.text(&["rev-parse", "--absolute-git-dir"])?);
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(gitdir.join("git-sync.lock"))
        .map_err(|e| e.to_string())?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|_| "该仓库已有同步进程正在运行".to_string())?;
    validate(&git, task, offline)?;
    let heads = git.text(&["ls-remote", "--heads", "origin"])?;
    let reference = format!("refs/heads/{}", task.branch);
    let remote_exists = heads
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(reference.as_str()));
    if remote_exists {
        git.text(&["fetch", "--no-tags", "origin", &task.branch])?;
    } else if !heads.is_empty() {
        return Err(blocked("远程分支不存在，请检查任务中的分支名称"));
    }
    let before = git.fingerprint()?;
    thread::sleep(Duration::from_secs(settle));
    if before != git.fingerprint()? {
        return Ok("busy");
    }
    git.text(&["add", "-A", "--", "."])?;
    match git.call(&["diff", "--cached", "--quiet"])?.0 {
        0 => {}
        1 => {
            git.text(&["commit", "-m", "Git sync: local edits"])?;
        }
        _ => return Err("无法检查暂存区内容".to_string().into()),
    }
    if !git.text(&["status", "--porcelain"])?.is_empty() {
        return Ok("busy");
    }
    if !remote_exists {
        if git.call(&["rev-parse", "--verify", "HEAD"])?.0 != 0 {
            return Ok("unchanged");
        }
        git.text(&[
            "push",
            "origin",
            &format!("HEAD:refs/heads/{}", task.branch),
        ])?;
        return Ok("synced");
    }
    let local = git.text(&["rev-parse", "HEAD"])?;
    let remote = git.text(&["rev-parse", "FETCH_HEAD"])?;
    if local == remote {
        return Ok("unchanged");
    }
    if git.call(&["merge-base", "HEAD", &remote])?.0 != 0 {
        return Err(blocked("本地与远程历史无共同起点，已停止自动合并"));
    }
    if git.call(&["merge", "--no-edit", &remote])?.0 != 0 {
        let files = git.text(&["diff", "--name-only", "--diff-filter=U"])?;
        return Err(blocked(format!(
            "合并已暂停，请手动解决冲突或中止合并后再恢复。{files}"
        )));
    }
    if !git.text(&["status", "--porcelain"])?.is_empty() {
        return Ok("busy");
    }
    if git.text(&["rev-parse", "HEAD"])? != remote {
        git.text(&[
            "push",
            "origin",
            &format!("HEAD:refs/heads/{}", task.branch),
        ])?;
    }
    Ok("synced")
}
