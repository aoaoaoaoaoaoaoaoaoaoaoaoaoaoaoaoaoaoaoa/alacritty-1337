use log::{info, warn};
use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString, c_void};
use std::io::{Error, ErrorKind, Result};
use std::mem::{self, size_of};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle};
use std::ptr;

use windows_sys::Win32::Foundation::{FreeLibrary, HANDLE, HMODULE, S_OK};
use windows_sys::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::core::HRESULT;
use windows_sys::{s, w};

use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    UpdateProcThreadAttribute,
};

use crate::event::{OnResize, WindowSize};
use crate::tty::Options;
use crate::tty::windows::blocking::{UnblockedReader, UnblockedWriter};
use crate::tty::windows::child::ChildExitWatcher;
use crate::tty::windows::{Pty, application, cmdline};

const PIPE_CAPACITY: usize = crate::event_loop::READ_BUFFER_SIZE;

/// Load the pseudoconsole API from conpty.dll if possible, otherwise use the
/// standard Windows API.
///
/// The conpty.dll from the Windows Terminal project
/// supports loading OpenConsole.exe, which offers many improvements and
/// bugfixes compared to the standard conpty that ships with Windows.
///
/// The conpty.dll and OpenConsole.exe files will be searched in PATH and in
/// the directory where alacritty-1337's executable is located.
type CreatePseudoConsoleFn =
    unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> HRESULT;
type ResizePseudoConsoleFn = unsafe extern "system" fn(HPCON, COORD) -> HRESULT;
type ClosePseudoConsoleFn = unsafe extern "system" fn(HPCON);

struct ConptyApi {
    create: CreatePseudoConsoleFn,
    resize: ResizePseudoConsoleFn,
    close: ClosePseudoConsoleFn,
    _library: Option<DynamicLibrary>,
}

struct DynamicLibrary(HMODULE);

impl Drop for DynamicLibrary {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}

impl ConptyApi {
    fn new() -> Self {
        if let Some(conpty) = Self::load_conpty() {
            info!("Using conpty.dll for pseudoconsole");
            conpty
        } else {
            // Cannot load conpty.dll - use the standard Windows API.
            info!("Using Windows API for pseudoconsole");
            Self {
                create: CreatePseudoConsole,
                resize: ResizePseudoConsole,
                close: ClosePseudoConsole,
                _library: None,
            }
        }
    }

    /// Try loading `ConptyApi` from the `conpty.dll` library.
    fn load_conpty() -> Option<Self> {
        type LoadedFn = unsafe extern "system" fn() -> isize;
        unsafe {
            let hmodule = LoadLibraryW(w!("conpty.dll"));
            if hmodule.is_null() {
                return None;
            }
            let library = DynamicLibrary(hmodule);
            let create_fn = GetProcAddress(hmodule, s!("CreatePseudoConsole"))?;
            let resize_fn = GetProcAddress(hmodule, s!("ResizePseudoConsole"))?;
            let close_fn = GetProcAddress(hmodule, s!("ClosePseudoConsole"))?;

            Some(Self {
                create: mem::transmute::<LoadedFn, CreatePseudoConsoleFn>(create_fn),
                resize: mem::transmute::<LoadedFn, ResizePseudoConsoleFn>(resize_fn),
                close: mem::transmute::<LoadedFn, ClosePseudoConsoleFn>(close_fn),
                _library: Some(library),
            })
        }
    }
}

/// RAII Pseudoconsole.
pub struct Conpty {
    handle: HPCON,
    api: ConptyApi,
}

impl Drop for Conpty {
    fn drop(&mut self) {
        // XXX: This will block until the conout pipe is drained. Will cause a deadlock if the
        // conout pipe has already been dropped by this point.
        //
        // See PR #3084 and https://docs.microsoft.com/en-us/windows/console/closepseudoconsole.
        unsafe { (self.api.close)(self.handle) }
    }
}

// The ConPTY handle can be sent between threads.
unsafe impl Send for Conpty {}

struct ThreadAttributeList {
    _storage: Box<[u8]>,
    pointer: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl ThreadAttributeList {
    fn new() -> Result<Self> {
        let mut size = 0;
        let _ = unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &raw mut size) };
        if size == 0 {
            return Err(Error::last_os_error());
        }

        let mut storage = vec![0; size].into_boxed_slice();
        let pointer = storage.as_mut_ptr().cast();
        let success = unsafe { InitializeProcThreadAttributeList(pointer, 1, 0, &raw mut size) };
        if success == 0 {
            return Err(Error::last_os_error());
        }

        Ok(Self { _storage: storage, pointer })
    }

    fn attach_conpty(&mut self, handle: HPCON) -> Result<()> {
        let success = unsafe {
            UpdateProcThreadAttribute(
                self.pointer,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                handle as *mut c_void,
                size_of::<HPCON>(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if success == 0 { Err(Error::last_os_error()) } else { Ok(()) }
    }
}

impl Drop for ThreadAttributeList {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.pointer);
        }
    }
}

pub fn new(config: &Options, window_size: WindowSize) -> Result<Pty> {
    let api = ConptyApi::new();
    let mut pty_handle: HPCON = 0;

    // Passing 0 as the size parameter allows the "system default" buffer
    // size to be used. There may be small performance and memory advantages
    // to be gained by tuning this in the future, but it's likely a reasonable
    // start point.
    let (conout, conout_pty_handle) = miow::pipe::anonymous(0)?;
    let (conin_pty_handle, conin) = miow::pipe::anonymous(0)?;
    let conout_pty_handle =
        unsafe { OwnedHandle::from_raw_handle(conout_pty_handle.into_raw_handle()) };
    let conin_pty_handle =
        unsafe { OwnedHandle::from_raw_handle(conin_pty_handle.into_raw_handle()) };

    // Create the Pseudo Console, using the pipes.
    let result = unsafe {
        (api.create)(
            window_size.into(),
            conin_pty_handle.as_raw_handle(),
            conout_pty_handle.as_raw_handle(),
            0,
            &raw mut pty_handle,
        )
    };

    if result != S_OK {
        return Err(Error::other(format!("CreatePseudoConsole failed with HRESULT {result:#x}")));
    }
    let conpty = Conpty { handle: pty_handle, api };

    // Prepare child process startup info.

    let mut startup_info_ex: STARTUPINFOEXW = unsafe { mem::zeroed() };

    startup_info_ex.StartupInfo.lpTitle = ptr::null_mut();

    startup_info_ex.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;

    // Setting this flag but leaving all the handles as default (null) ensures the
    // PTY process does not inherit any handles from this alacritty-1337 process.
    startup_info_ex.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;

    let mut attr_list = ThreadAttributeList::new()?;
    attr_list.attach_conpty(conpty.handle)?;
    startup_info_ex.lpAttributeList = attr_list.pointer;

    // Prepare child process creation arguments.
    let application = checked_win32_string(OsStr::new(application(config)))?;
    let mut cmdline = checked_win32_string(OsStr::new(&cmdline(config)))?;
    let cwd = config
        .working_directory
        .as_ref()
        .map(|path| checked_win32_string(path.as_os_str()))
        .transpose()?;
    let mut creation_flags = EXTENDED_STARTUPINFO_PRESENT;
    let custom_env_block = convert_custom_env(&config.env)?;
    let custom_env_block_pointer = match &custom_env_block {
        Some(custom_env_block) => {
            creation_flags |= CREATE_UNICODE_ENVIRONMENT;
            custom_env_block.as_ptr().cast::<c_void>().cast_mut()
        },
        None => ptr::null_mut(),
    };

    let mut proc_info: PROCESS_INFORMATION = unsafe { mem::zeroed() };
    unsafe {
        let success = CreateProcessW(
            application.as_ptr(),
            cmdline.as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            i32::from(false),
            creation_flags,
            custom_env_block_pointer,
            cwd.as_ref().map_or_else(ptr::null, Vec::as_ptr),
            &raw mut startup_info_ex.StartupInfo,
            &raw mut proc_info,
        );

        if success == 0 {
            return Err(Error::last_os_error());
        }
    }

    let process_handle = unsafe { OwnedHandle::from_raw_handle(proc_info.hProcess) };
    let thread_handle = unsafe { OwnedHandle::from_raw_handle(proc_info.hThread) };
    drop(thread_handle);

    let conin = UnblockedWriter::new(conin, PIPE_CAPACITY);
    let conout = UnblockedReader::new(conout, PIPE_CAPACITY);

    let child_watcher = ChildExitWatcher::new(process_handle)?;

    Ok(Pty::new(conpty, conout, conin, child_watcher))
}

// Windows environment variables are case-insensitive, and the caller is responsible for
// deduplicating environment variables, so do that here while converting.
//
// https://learn.microsoft.com/en-us/previous-versions/troubleshoot/windows/win32/createprocess-cannot-eliminate-duplicate-variables#environment-variables
fn convert_custom_env(custom_env: &HashMap<String, String>) -> Result<Option<Vec<u16>>> {
    // Windows inherits parent's env when no `lpEnvironment` parameter is specified.
    if custom_env.is_empty() {
        return Ok(None);
    }

    let mut environment = BTreeMap::new();
    for (inherited_key, inherited_value) in std::env::vars_os() {
        insert_environment(&mut environment, &inherited_key, &inherited_value)?;
    }
    // Configuration wins over inherited variables, case-insensitively.
    for (key, value) in custom_env {
        if key.is_empty() || key.contains('=') {
            return Err(Error::new(ErrorKind::InvalidInput, "invalid environment variable name"));
        }
        insert_environment(&mut environment, OsStr::new(key), OsStr::new(value))?;
    }

    let mut converted_block = Vec::new();
    for (_, (key, value)) in environment {
        converted_block.extend(key);
        converted_block.push('=' as u16);
        converted_block.extend(value);
        converted_block.push(0);
    }
    converted_block.push(0);
    Ok(Some(converted_block))
}

// According to the `lpEnvironment` parameter description:
// https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessa#parameters
//
// > An environment block consists of a null-terminated block of null-terminated strings. Each
// string is in the following form:
// >
// > name=value\0
fn insert_environment(
    environment: &mut BTreeMap<Vec<u16>, (Vec<u16>, Vec<u16>)>,
    key: &OsStr,
    value: &OsStr,
) -> Result<()> {
    let key = encode_without_nul(key)?;
    let value = encode_without_nul(value)?;
    let folded = OsString::from_wide(&key).to_ascii_uppercase().encode_wide().collect();
    let _ = environment.insert(folded, (key, value));
    Ok(())
}

fn checked_win32_string(value: &OsStr) -> Result<Vec<u16>> {
    let mut encoded = encode_without_nul(value)?;
    encoded.push(0);
    Ok(encoded)
}

fn encode_without_nul(value: &OsStr) -> Result<Vec<u16>> {
    let encoded = value.encode_wide().collect::<Vec<_>>();
    if encoded.contains(&0) {
        Err(Error::new(ErrorKind::InvalidInput, "embedded NUL in Windows process input"))
    } else {
        Ok(encoded)
    }
}

impl OnResize for Conpty {
    fn on_resize(&mut self, window_size: WindowSize) {
        let result = unsafe { (self.api.resize)(self.handle, window_size.into()) };
        if result != S_OK {
            warn!("ResizePseudoConsole failed with HRESULT {result:#x}");
        }
    }
}

impl From<WindowSize> for COORD {
    fn from(window_size: WindowSize) -> Self {
        let lines = window_size.num_lines;
        let columns = window_size.num_cols;
        COORD { X: columns.min(i16::MAX as u16) as i16, Y: lines.min(i16::MAX as u16) as i16 }
    }
}
