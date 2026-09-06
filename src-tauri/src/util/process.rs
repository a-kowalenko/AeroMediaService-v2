//! Helpers for spawning OS helper processes without flashing a console on Windows.

use std::ffi::OsStr;
use std::process::Command;

/// Build a `Command` that does not allocate a visible console window on Windows.
///
/// GUI apps (`windows` subsystem) spawn console-subsystem children (`net`, `ipconfig`,
/// `powershell`, `ffmpeg`, …) with a brief black console flash unless
/// `CREATE_NO_WINDOW` is set. Use this for every background helper process.
pub fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}
