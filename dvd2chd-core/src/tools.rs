//! Tool-Discovery und Version-Ausgabe für chdman/ddrescue/cdrdao

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct Tools {
    pub chdman: Option<ToolInfo>,
    pub ddrescue: Option<ToolInfo>,
    pub cdrdao: Option<ToolInfo>,
}

#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub path: PathBuf,
    pub version: Option<String>,
}

pub fn probe_tool(name: &str, override_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = override_path {
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    which::which(name).ok()
}

pub fn tool_version(path: &Path, version_arg: &str) -> Option<String> {
    let mut cmd = Command::new(path);
    cmd.arg(version_arg);
    crate::util::hide_console_window(&mut cmd);
    let out = cmd.output().ok()?;
    let take_first = |bytes: &[u8]| {
        let s = String::from_utf8_lossy(bytes);
        let line = s.lines().next().unwrap_or_default().trim();
        if line.is_empty() {
            None
        } else {
            Some(line.to_string())
        }
    };
    if out.status.success() {
        take_first(&out.stdout).or_else(|| take_first(&out.stderr))
    } else {
        None
    }
}

pub fn probe_all(
    override_chdman: Option<&Path>,
    override_ddrescue: Option<&Path>,
    override_cdrdao: Option<&Path>,
) -> Tools {
    let chdman = probe_tool("chdman", override_chdman).map(|p| ToolInfo {
        version: tool_version(&p, "-version").or_else(|| tool_version(&p, "--version")),
        path: p,
    });
    let ddrescue = probe_tool("ddrescue", override_ddrescue).map(|p| ToolInfo {
        version: tool_version(&p, "--version"),
        path: p,
    });
    let cdrdao = probe_tool("cdrdao", override_cdrdao).map(|p| ToolInfo {
        version: tool_version(&p, "--version"),
        path: p,
    });
    Tools {
        chdman,
        ddrescue,
        cdrdao,
    }
}
