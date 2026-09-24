use crate::{
    credentials,
    engine::{self, Git},
    model::{Catalog, Result, Task},
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

// A failed setup never replaces a user's working file or modifies an existing repository.
pub fn probe(path: &str, dir: &Path) -> Result<Option<(String, String, String)>> {
    let target = destination(path)?;
    if target.join(".git").exists() {
        return engine::inspect(target.to_string_lossy().as_ref(), dir).map(Some);
    }
    let mut ancestor = target.as_path();
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or("无效的本地目录")?;
    }
    if !ancestor.is_dir() {
        return Err("本地路径指向文件，请填写目录".into());
    }
    let git = Git::new(ancestor, dir, None, "github");
    if git.call(&["rev-parse", "--git-dir"])?.0 == 0 {
        return Err("该路径位于其他 Git 仓库中，请选择仓库根目录或独立目录".into());
    }
    Ok(None)
}
fn destination(path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() {
        return Err("请输入完整的本地目录路径".into());
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("请使用不含 .. 的完整路径".into());
    }
    Ok(path)
}
pub fn non_empty(path: &str) -> Result<bool> {
    let target = destination(path)?;
    if !target.exists() {
        return Ok(false);
    }
    Ok(fs::read_dir(target)
        .map_err(|e| e.to_string())?
        .next()
        .transpose()
        .map_err(|e| e.to_string())?
        .is_some())
}
pub fn prepare(
    task: &Task,
    catalog: &Catalog,
    dir: &Path,
    offline: bool,
    confirm_non_empty: bool,
) -> Result<()> {
    if probe(&task.local_dir, dir)?.is_some() {
        return Ok(());
    }
    if !offline && task.provider != "github" {
        return Err("Overleaf 请先选择已克隆的 Git 仓库".into());
    }
    let target = destination(&task.local_dir)?;
    if non_empty(&task.local_dir)? && !confirm_non_empty {
        return Err("目录非空，请确认后再创建同步".into());
    }
    let credential = if offline {
        None
    } else {
        credentials::selected(dir, task, catalog)?
    };
    // Fetch in a disposable directory before creating or modifying the destination.
    let stage = tempfile::Builder::new()
        .prefix("git-sync-prepare-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let git = Git::new(stage.path(), dir, credential, &task.provider);
    git.text(&["check-ref-format", "--branch", &task.branch])?;
    git.text(&["init", "--initial-branch", &task.branch])?;
    git.text(&["remote", "add", "origin", &task.remote_url])?;
    let heads = git.text(&["ls-remote", "--heads", "origin"])?;
    let reference = format!("refs/heads/{}", task.branch);
    let exists = heads
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(reference.as_str()));
    if exists {
        git.text(&["fetch", "--no-tags", "origin", &task.branch])?;
        git.text(&["checkout", "-B", &task.branch, "FETCH_HEAD"])?;
    } else if !heads.is_empty() {
        return Err(format!(
            "远程不存在分支 {}，请填写远程已有的分支名称",
            task.branch
        ));
    }
    check_copy(stage.path(), &target)?;
    // Recheck after the network operation: the user may have initialized the folder meanwhile.
    if probe(&task.local_dir, dir)?.is_some() {
        return Err("目录已成为 Git 仓库，请重新读取仓库信息".into());
    }
    if non_empty(&task.local_dir)? && !confirm_non_empty {
        return Err("目录在准备期间新增了文件，请重新确认后再创建".into());
    }
    fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    copy_missing(stage.path(), &target)?;
    // Move .git across volumes by copying it to a temporary directory on the destination volume.
    // This makes the final attachment a same-volume rename; existing .git is never overwritten.
    let metadata = tempfile::Builder::new()
        .prefix(".git-sync-metadata-")
        .tempdir_in(&target)
        .map_err(|e| e.to_string())?;
    copy_metadata(&stage.path().join(".git"), metadata.path())?;
    if target.join(".git").exists() {
        return Err("目录的 Git 状态已变化，请重新读取仓库信息".into());
    }
    fs::rename(metadata.path(), target.join(".git")).map_err(|e| e.to_string())?;
    Ok(())
}
fn check_copy(from: &Path, to: &Path) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(to) {
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(format!("路径不是普通目录：{}", to.display()));
        }
    }
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name() == ".git" {
            continue;
        }
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let dest = to.join(entry.file_name());
        if kind.is_symlink() {
            return Err("仓库包含符号链接，请先使用 Git 手动克隆".into());
        }
        if kind.is_dir() {
            check_copy(&entry.path(), &dest)?;
        } else if let Ok(meta) = fs::symlink_metadata(&dest) {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(format!("本地与远程的文件/目录类型冲突：{}", dest.display()));
            }
        }
    }
    Ok(())
}
fn copy_missing(from: &Path, to: &Path) -> Result<()> {
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name() == ".git" {
            continue;
        }
        let dest = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            if let Ok(meta) = fs::symlink_metadata(&dest) {
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err("目录在初始化期间发生变化，请重试".into());
                }
            }
            fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            copy_missing(&entry.path(), &dest)?;
        } else {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dest)
            {
                Ok(mut output) => {
                    let mut input = fs::File::open(entry.path()).map_err(|e| e.to_string())?;
                    io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    }
    Ok(())
}
fn copy_metadata(from: &Path, to: &Path) -> Result<()> {
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let dest = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            fs::create_dir(&dest).map_err(|e| e.to_string())?;
            copy_metadata(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
