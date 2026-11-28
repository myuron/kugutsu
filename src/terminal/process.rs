//! PTY のフォアグラウンドで動いているプロセスを調べる。

use std::fs::File;

/// フォアグラウンドのプロセスグループのリーダー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    /// カーネルが保持するプロセス名（Linux は `/proc/<pid>/comm`、macOS は `proc_name`）。
    pub comm: String,
    /// コマンドライン引数（Linux は `/proc/<pid>/cmdline`、macOS は `KERN_PROCARGS2`）。
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

/// PTY の master 側からフォアグラウンドのプロセスを得る。
#[cfg(target_os = "macos")]
pub fn foreground_process(master: &File) -> Option<ProcessInfo> {
    use std::os::fd::AsRawFd;

    // SAFETY: master は開いている PTY の fd で、tcgetpgrp は fd を読むだけ
    let pgid = unsafe { libc::tcgetpgrp(master.as_raw_fd()) };
    if pgid <= 0 {
        return None;
    }

    let mut name = [0u8; 2 * libc::MAXCOMLEN + 1];
    // SAFETY: name は buffersize バイトの書き込める領域
    let len = unsafe { libc::proc_name(pgid, name.as_mut_ptr().cast(), name.len() as u32) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0)?;
    let comm = String::from_utf8_lossy(&name[..len]).into_owned();

    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pgid];
    let mut size: libc::size_t = 0;
    // SAFETY: oldp が NULL なので、必要なバイト数を size に書くだけ
    let ret = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 || size == 0 {
        return None;
    }
    let mut args = vec![0u8; size];
    // SAFETY: args は size バイトの書き込める領域で、書いたバイト数が size に返る
    let ret = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            args.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return None;
    }
    args.truncate(size);

    Some(ProcessInfo {
        comm,
        argv: parse_procargs2(&args)?,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn foreground_process(_master: &File) -> Option<ProcessInfo> {
    None
}

/// NUL 区切りのコマンドラインを引数に分ける。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_cmdline(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|&byte| byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect()
}

/// `sysctl(KERN_PROCARGS2)` のバッファからコマンドライン引数を取り出す。
///
/// バッファは `argc`（i32）、実行ファイルのパス、NUL の詰め物、`argc` 個の NUL 終端の引数、
/// 環境変数の順に並ぶ。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_procargs2(bytes: &[u8]) -> Option<Vec<String>> {
    let (argc, rest) = bytes.split_first_chunk::<4>()?;
    let argc = usize::try_from(i32::from_ne_bytes(*argc)).ok()?;
    // 実行ファイルのパスと、それに続く NUL の詰め物を飛ばす
    let path_end = rest.iter().position(|&byte| byte == 0)?;
    let rest = &rest[path_end..];
    let start = rest
        .iter()
        .position(|&byte| byte != 0)
        .unwrap_or(rest.len());
    let mut args = rest[start..].split(|&byte| byte == 0);
    let argv = (0..argc)
        .map(|_| {
            args.next()
                .map(|arg| String::from_utf8_lossy(arg).into_owned())
        })
        .collect::<Option<Vec<_>>>()?;
    // 最後の引数が NUL で終わっていなければ切れている
    args.next()?;
    Some(argv)
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

    fn procargs2(argc: i32, rest: &[u8]) -> Vec<u8> {
        let mut bytes = argc.to_ne_bytes().to_vec();
        bytes.extend_from_slice(rest);
        bytes
    }

    #[test]
    fn parses_procargs2() {
        let bytes = procargs2(
            2,
            b"/usr/local/bin/node\0\0\0\0node\0/usr/local/bin/claude\0HOME=/Users/me\0",
        );
        assert_eq!(
            parse_procargs2(&bytes).unwrap(),
            ["node", "/usr/local/bin/claude"]
        );
    }

    #[test]
    fn parses_procargs2_without_arguments() {
        let bytes = procargs2(0, b"/bin/sleep\0\0HOME=/Users/me\0");
        assert!(parse_procargs2(&bytes).unwrap().is_empty());
    }

    #[test]
    fn rejects_truncated_procargs2() {
        assert_eq!(parse_procargs2(b"\x01\0"), None);
        assert_eq!(
            parse_procargs2(&procargs2(2, b"/bin/sleep\0\0sleep\0")),
            None
        );
        assert_eq!(parse_procargs2(&procargs2(-1, b"/bin/sleep\0")), None);
    }
}
