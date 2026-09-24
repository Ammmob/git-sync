use crate::model::{Catalog, Result, Task};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn hidden(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}
pub fn token_path(dir: &Path, id: &str) -> Result<PathBuf> {
    if id.len() != 32 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("无效的 Token ID".into());
    }
    Ok(dir.join("credentials").join(format!("{id}.xml")))
}
pub fn init(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir.join("credentials")).map_err(|e| e.to_string())?;
    fs::create_dir_all(dir.join("helpers")).map_err(|e| e.to_string())?;
    fs::write(
        dir.join("helpers/askpass.ps1"),
        r#"param([string]$Prompt)
$ErrorActionPreference = 'Stop'
if ($Prompt -match 'Username') { [Console]::Out.Write($env:GIT_SYNC_USERNAME); exit }
$secret = Import-Clixml -LiteralPath $env:GIT_SYNC_CREDENTIAL
$ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secret)
try { [Console]::Out.Write([Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr)) }
finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr) }
"#,
    )
    .map_err(|e| e.to_string())?;
    fs::write(dir.join("helpers/askpass.cmd"), "@echo off\r\npowershell.exe -NoProfile -ExecutionPolicy Bypass -File \"%~dp0askpass.ps1\" %*\r\n").map_err(|e| e.to_string())?;
    Ok(())
}
pub fn save(dir: &Path, id: &str, value: &str) -> Result<()> {
    let path = token_path(dir, id)?;
    let mut command = Command::new("powershell.exe");
    hidden(&mut command).args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; $v=[Console]::In.ReadToEnd(); ConvertTo-SecureString -String $v -AsPlainText -Force | Export-Clixml -LiteralPath $env:GIT_SYNC_CREDENTIAL"])
        .env("GIT_SYNC_CREDENTIAL", &path).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = command.spawn().map_err(|_| "无法保存 Token".to_string())?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(value.as_bytes())
        .map_err(|_| "无法保存 Token".to_string())?;
    use wait_timeout::ChildExt;
    match child
        .wait_timeout(std::time::Duration::from_secs(20))
        .map_err(|e| e.to_string())?
    {
        Some(s) if s.success() => Ok(()),
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            Err("Token 保存失败，请重试".into())
        }
    }
}
pub fn selected(dir: &Path, task: &Task, catalog: &Catalog) -> Result<Option<PathBuf>> {
    let id = task.token_id.as_ref().or_else(|| {
        catalog
            .defaults
            .get(&task.provider)
            .and_then(|v| v.as_ref())
    });
    match id {
        None => Ok(None),
        Some(id) if id == "@system" => Ok(None),
        Some(id) => {
            if !catalog
                .tokens
                .iter()
                .any(|t| &t.id == id && t.provider == task.provider)
            {
                return Err("所选 Token 不可用，请重新选择".into());
            }
            let path = token_path(dir, id)?;
            if !path.exists() {
                return Err("所选 Token 文件缺失，请重新添加".into());
            }
            Ok(Some(path))
        }
    }
}
