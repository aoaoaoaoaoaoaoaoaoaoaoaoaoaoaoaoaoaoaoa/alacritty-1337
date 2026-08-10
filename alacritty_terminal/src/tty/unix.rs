//! TTY related functionality.

use std::env;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{Error, ErrorKind, Read, Result};
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
#[cfg(target_os = "macos")]
use std::path::Path;
use std::process::{Child, Command};
use std::sync::Arc;

use libc::{F_GETFL, F_SETFL, O_NONBLOCK, TIOCSCTTY, c_int, fcntl};
use log::error;
use polling::{Event, PollMode, Poller};
use rustix::termios::Winsize;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use rustix::termios::{self, InputModes, OptionalActions};
use rustix_openpty::openpty;
use signal_hook::low_level::{pipe as signal_pipe, unregister as unregister_signal};
use signal_hook::{SigId, consts as sigconsts};

use crate::event::{OnResize, WindowSize};
use crate::tty::{ChildEvent, EventedPty, EventedReadWrite, Options};

// Interest in PTY read/writes.
pub(crate) const PTY_READ_WRITE_TOKEN: usize = 0;

// Interest in new child events.
pub(crate) const PTY_CHILD_EVENT_TOKEN: usize = 1;

/// Really only needed on BSD, but should be fine elsewhere.
fn set_controlling_terminal(fd: c_int) -> Result<()> {
    let res = unsafe {
        // The request constant's width varies by platform; `ioctl` requires `c_ulong`.
        libc::ioctl(fd, libc::c_ulong::from(TIOCSCTTY), 0)
    };

    if res == 0 { Ok(()) } else { Err(Error::last_os_error()) }
}

#[derive(Debug)]
struct Passwd<'a> {
    name: &'a str,
    dir: &'a str,
    shell: &'a str,
}

/// Return a Passwd struct with pointers into the provided buf.
///
/// # Unsafety
///
/// If `buf` is changed while `Passwd` is alive, bad thing will almost certainly happen.
fn get_pw_entry(buf: &mut Vec<libc::c_char>) -> Result<Passwd<'_>> {
    let uid = unsafe { libc::getuid() };

    loop {
        let mut entry = MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(uid, entry.as_mut_ptr(), buf.as_mut_ptr(), buf.len(), &raw mut result)
        };

        if status == libc::ERANGE {
            let new_len =
                buf.len().checked_mul(2).ok_or_else(|| Error::other("passwd buffer overflow"))?;
            buf.try_reserve_exact(new_len - buf.len()).map_err(Error::other)?;
            buf.resize(new_len, 0);
            continue;
        }
        if status != 0 {
            return Err(Error::from_raw_os_error(status));
        }
        if result.is_null() {
            return Err(Error::new(ErrorKind::NotFound, "passwd entry not found"));
        }

        let entry = unsafe { entry.assume_init() };
        assert_eq!(entry.pw_uid, uid);

        let invalid_utf8 = |_| Error::new(ErrorKind::InvalidData, "passwd entry is not UTF-8");
        return Ok(Passwd {
            name: unsafe { CStr::from_ptr(entry.pw_name) }.to_str().map_err(invalid_utf8)?,
            dir: unsafe { CStr::from_ptr(entry.pw_dir) }.to_str().map_err(invalid_utf8)?,
            shell: unsafe { CStr::from_ptr(entry.pw_shell) }.to_str().map_err(invalid_utf8)?,
        });
    }
}

pub struct Pty {
    child: Child,
    file: File,
    signals: UnixStream,
    sig_id: SigId,
}

impl Pty {
    pub fn child(&self) -> &Child {
        &self.child
    }

    pub fn file(&self) -> &File {
        &self.file
    }
}

/// User information that is required for a new shell session.
struct ShellUser {
    user: String,
    home: String,
    shell: String,
}

impl ShellUser {
    /// look for shell, username, longname, and home dir in the respective environment variables
    /// before falling back on looking into `passwd`.
    fn from_env() -> Result<Self> {
        let mut buf = vec![0; 1024];
        let pw = get_pw_entry(&mut buf);
        let passwd = || pw.as_ref().map_err(|err| Error::new(err.kind(), err.to_string()));

        let user = match env::var("USER") {
            Ok(user) => user,
            Err(_) => passwd()?.name.to_owned(),
        };

        let home = match env::var("HOME") {
            Ok(home) => home,
            Err(_) => passwd()?.dir.to_owned(),
        };

        let shell = match env::var("SHELL") {
            Ok(shell) => shell,
            Err(_) => passwd()?.shell.to_owned(),
        };

        Ok(Self { user, home, shell })
    }
}

#[cfg(not(target_os = "macos"))]
fn default_shell_command(shell: &str, _user: &str, _home: &str) -> Command {
    Command::new(shell)
}

#[cfg(target_os = "macos")]
fn default_shell_command(shell: &str, user: &str, home: &str) -> Command {
    let shell_name = shell.rsplit('/').next().unwrap_or(shell);

    // On macOS, use the `login` command so the shell will appear as a tty session.
    let mut login_command = Command::new("/usr/bin/login");

    // Exec the shell with argv[0] prepended by '-' so it becomes a login shell.
    // `login` normally does this itself, but `-l` disables this.
    let exec = format!("exec -a -{shell_name} {shell}");

    // Since we use -l, `login` will not change directory to the user's home. However,
    // `login` only checks the current working directory for a .hushlogin file, causing
    // it to miss any in the user's home directory. We can fix this by doing the check
    // ourselves and passing `-q`
    let has_home_hushlogin = Path::new(home).join(".hushlogin").exists();

    // -f: Bypasses authentication for the already-logged-in user.
    // -l: Skips changing directory to $HOME and prepending '-' to argv[0].
    // -p: Preserves the environment.
    // -q: Act as if `.hushlogin` exists.
    //
    // XXX: we use zsh here over sh due to `exec -a`.
    let flags = if has_home_hushlogin { "-qflp" } else { "-flp" };
    let _ = login_command.args([flags, user, "/bin/zsh", "-fc", &exec]);
    login_command
}

/// Create a new TTY and return a handle to interact with it.
pub fn new(config: &Options, window_size: WindowSize, window_id: u64) -> Result<Pty> {
    let pty = openpty(None, Some(&window_size.to_winsize()))?;
    let (master, slave) = (pty.controller, pty.user);
    from_fd(config, window_id, master, slave)
}

/// Create a new TTY from a PTY's file descriptors.
pub fn from_fd(config: &Options, window_id: u64, master: OwnedFd, slave: OwnedFd) -> Result<Pty> {
    let master_fd = master.as_raw_fd();
    let slave_fd = slave.as_raw_fd();

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Ok(mut termios) = termios::tcgetattr(&master) {
        // Set character encoding to UTF-8.
        termios.input_modes.set(InputModes::IUTF8, true);
        let _ = termios::tcsetattr(&master, OptionalActions::Now, &termios);
    }

    let user = ShellUser::from_env()?;

    let mut builder = if let Some(shell) = config.shell.as_ref() {
        let mut cmd = Command::new(&shell.program);
        let _ = cmd.args(shell.args.as_slice());
        cmd
    } else {
        default_shell_command(&user.shell, &user.user, &user.home)
    };

    // Setup child stdin/stdout/stderr as slave fd of PTY.
    let _ = builder.stdin(slave.try_clone()?).stderr(slave.try_clone()?).stdout(slave);

    // Setup shell environment.
    let window_id = window_id.to_string();
    let _ = builder
        .env("ALACRITTY_WINDOW_ID", &window_id)
        .env("USER", user.user)
        .env("HOME", user.home);
    // Set Window ID for clients relying on X11 hacks.
    let _ = builder.env("WINDOWID", window_id);
    for (key, value) in &config.env {
        let _ = builder.env(key, value);
    }

    // Prevent child processes from inheriting linux-specific startup notification env.
    let _ = builder.env_remove("XDG_ACTIVATION_TOKEN").env_remove("DESKTOP_STARTUP_ID");

    let working_directory = config
        .working_directory
        .as_ref()
        .and_then(|path| CString::new(path.as_os_str().as_bytes()).ok());

    unsafe {
        let _ = builder.pre_exec(move || {
            // Create a new process group.
            let err = libc::setsid();
            if err == -1 {
                return Err(Error::last_os_error());
            }

            // Set working directory, ignoring invalid paths.
            if let Some(working_directory) = working_directory.as_ref()
                && libc::chdir(working_directory.as_ptr()) == -1
            {
                return Err(Error::last_os_error());
            }

            set_controlling_terminal(slave_fd)?;

            // No longer need slave/master fds.
            let _ = libc::close(slave_fd);
            let _ = libc::close(master_fd);

            let _ = libc::signal(libc::SIGCHLD, libc::SIG_DFL);
            let _ = libc::signal(libc::SIGHUP, libc::SIG_DFL);
            let _ = libc::signal(libc::SIGINT, libc::SIG_DFL);
            let _ = libc::signal(libc::SIGQUIT, libc::SIG_DFL);
            let _ = libc::signal(libc::SIGTERM, libc::SIG_DFL);
            let _ = libc::signal(libc::SIGALRM, libc::SIG_DFL);

            Ok(())
        });
    }

    // Set nonblocking before spawning so every post-spawn failure owns and reaps a child.
    unsafe {
        set_nonblocking(master_fd)?;
    }

    // Prepare signal handling before spawning child.
    let (signals, sig_id) = {
        let (sender, recv) = UnixStream::pair()?;

        // Register the recv end of the pipe for SIGCHLD.
        let sig_id = signal_pipe::register(sigconsts::SIGCHLD, sender)?;
        if let Err(err) = recv.set_nonblocking(true) {
            let _ = unregister_signal(sig_id);
            return Err(err);
        }
        (recv, sig_id)
    };

    match builder.spawn() {
        Ok(child) => Ok(Pty { child, file: File::from(master), signals, sig_id }),
        Err(err) => {
            let _ = unregister_signal(sig_id);
            Err(Error::new(
                err.kind(),
                format!(
                    "Failed to spawn command '{}': {}",
                    builder.get_program().to_string_lossy(),
                    err
                ),
            ))
        },
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // An unreaped child retains its PID, so a successful live check closes the reuse window.
        match self.child.try_wait() {
            Ok(None) => unsafe {
                if libc::kill(self.child.id() as i32, libc::SIGHUP) == -1 {
                    error!("Unable to signal PTY child: {}", Error::last_os_error());
                }
            },
            Ok(Some(_)) => (),
            Err(err) => error!("Unable to inspect PTY child before shutdown: {err}"),
        }

        // Clear signal-hook handler.
        let _ = unregister_signal(self.sig_id);

        let _ = self.child.wait();
    }
}

impl EventedReadWrite for Pty {
    type Reader = File;
    type Writer = File;

    #[inline]
    unsafe fn register(
        &mut self,
        poll: &Arc<Poller>,
        mut interest: Event,
        poll_opts: PollMode,
    ) -> Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        unsafe {
            poll.add_with_mode(&self.file, interest, poll_opts)?;
        }

        unsafe {
            poll.add_with_mode(
                &self.signals,
                Event::readable(PTY_CHILD_EVENT_TOKEN),
                PollMode::Level,
            )
        }
    }

    #[inline]
    fn reregister(
        &mut self,
        poll: &Arc<Poller>,
        mut interest: Event,
        poll_opts: PollMode,
    ) -> Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        poll.modify_with_mode(&self.file, interest, poll_opts)?;

        poll.modify_with_mode(
            &self.signals,
            Event::readable(PTY_CHILD_EVENT_TOKEN),
            PollMode::Level,
        )
    }

    #[inline]
    fn deregister(&mut self, poll: &Arc<Poller>) -> Result<()> {
        poll.delete(&self.file)?;
        poll.delete(&self.signals)
    }

    #[inline]
    fn reader(&mut self) -> &mut File {
        &mut self.file
    }

    #[inline]
    fn writer(&mut self) -> &mut File {
        &mut self.file
    }
}

impl EventedPty for Pty {
    #[inline]
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        // See if there has been a SIGCHLD.
        let mut buf = [0u8; 1];
        if let Err(err) = self.signals.read(&mut buf) {
            if err.kind() != ErrorKind::WouldBlock {
                error!("Error reading from signal pipe: {err}");
            }
            return None;
        }

        // Match on the child process.
        match self.child.try_wait() {
            Err(err) => {
                error!("Error checking child process termination: {err}");
                None
            },
            Ok(None) => None,
            Ok(exit_status) => Some(ChildEvent::Exited(exit_status)),
        }
    }
}

impl OnResize for Pty {
    /// Resize the PTY.
    ///
    /// Tells the kernel that the window size changed with the new pixel
    /// dimensions and line/column counts.
    fn on_resize(&mut self, window_size: WindowSize) {
        let win = window_size.to_winsize();

        let res = unsafe { libc::ioctl(self.file.as_raw_fd(), libc::TIOCSWINSZ, &raw const win) };

        if res < 0 {
            error!("ioctl TIOCSWINSZ failed: {}", Error::last_os_error());
        }
    }
}

/// Types that can produce a `Winsize`.
pub trait ToWinsize {
    /// Get a `Winsize`.
    fn to_winsize(self) -> Winsize;
}

impl ToWinsize for WindowSize {
    fn to_winsize(self) -> Winsize {
        let ws_row = self.num_lines as libc::c_ushort;
        let ws_col = self.num_cols as libc::c_ushort;

        let ws_xpixel = ws_col.saturating_mul(self.cell_width as libc::c_ushort);
        let ws_ypixel = ws_row.saturating_mul(self.cell_height as libc::c_ushort);
        Winsize { ws_row, ws_col, ws_xpixel, ws_ypixel }
    }
}

unsafe fn set_nonblocking(fd: c_int) -> Result<()> {
    let res = unsafe { fcntl(fd, F_SETFL, fcntl(fd, F_GETFL, 0) | O_NONBLOCK) };
    if res == 0 { Ok(()) } else { Err(Error::last_os_error()) }
}

#[test]
fn test_get_pw_entry() {
    let mut buf = vec![0];
    let _pw = get_pw_entry(&mut buf).unwrap();
    assert!(buf.len() > 1);
}
