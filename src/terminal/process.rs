//! PTY のフォアグラウンドで動いているプロセスを調べる。

use std::fs::File;

/// フォアグラウンドのプロセスグループのリーダー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    /// カーネルが保持するプロセス名（`/proc/<pid>/comm`）。
    pub comm: String,
    /// コマンドライン引数（`/proc/<pid>/cmdline`）。
    pub argv: Vec<String>,
}

/// PTY の master 側からフォアグラウンドのプロセスを得る。
#[cfg(target_os = "linux")]
pub fn foreground_process(master: &File) -> Option<ProcessInfo> {
    use std::os::fd::AsRawFd;

    // SAFETY: master は開いている PTY の fd で、tcgetpgrp は fd を読むだけ
    let pgid = unsafe { libc::tcgetpgrp(master.as_raw_fd()) };
    if pgid <= 0 {
        return None;
    }
    let comm = std::fs::read_to_string(format!("/proc/{pgid}/comm")).ok()?;
    let cmdline = std::fs::read(format!("/proc/{pgid}/cmdline")).ok()?;
    Some(ProcessInfo {
        comm: comm.trim_end().to_string(),
        argv: parse_cmdline(&cmdline),
    })
}

#[cfg(not(target_os = "linux"))]
pub fn foreground_process(_master: &File) -> Option<ProcessInfo> {
    None
}

/// NUL 区切りのコマンドラインを引数に分ける。
fn parse_cmdline(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|&byte| byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nul_separated_cmdline() {
        assert_eq!(
            parse_cmdline(b"node\0/usr/bin/claude\0"),
            ["node", "/usr/bin/claude"]
        );
        assert!(parse_cmdline(b"").is_empty());
    }
}
