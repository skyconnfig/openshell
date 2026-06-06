//! FreeRDP FFI bindings - safe Rust wrappers around the C shim API.
//!
//! The C shim (rdp_shim/shim.c) is compiled by build.rs into a static
//! library. These bindings expose its functions as a safe RdpConnection.
//!
//! When the `rdp` feature is disabled, stub types are provided so that
//! callers can refer to them without conditional compilation.

#[cfg(feature = "rdp")]
use std::ffi::{c_char, c_uchar, c_uint, c_ushort, c_void, CString};
#[cfg(feature = "rdp")]
use std::ptr::NonNull;
use std::fmt;

// ---------------------------------------------------------------------------
// Foreign function declarations (only available when FreeRDP is linked)
// ---------------------------------------------------------------------------

#[cfg(feature = "rdp")]
extern "C" {
    fn rdp_shim_connect(
        host: *const c_char,
        port: c_ushort,
        user: *const c_char,
        password: *const c_char,
        width: c_uint,
        height: c_uint,
    ) -> *mut c_void;

    fn rdp_shim_disconnect(conn: *mut c_void);

    fn rdp_shim_poll(conn: *mut c_void) -> bool;

    fn rdp_shim_framebuffer(
        conn: *mut c_void,
        width: *mut c_uint,
        height: *mut c_uint,
    ) -> *const c_uchar;

    fn rdp_shim_send_keyboard(conn: *mut c_void, scancode: c_ushort, down: bool);
    fn rdp_shim_send_mouse(conn: *mut c_void, flags: c_ushort, x: c_ushort, y: c_ushort);
    fn rdp_shim_resize(conn: *mut c_void, width: c_uint, height: c_uint);
}

// ---------------------------------------------------------------------------
// Safe wrapper
// ---------------------------------------------------------------------------

/// An active RDP connection. Calls rdp_shim_disconnect on drop.
pub struct RdpConnection {
    #[cfg(feature = "rdp")]
    inner: NonNull<c_void>,
}

/// The latest screen frame (RGBA, 4 bytes per pixel, top-left origin).
#[derive(Debug, Clone)]
pub struct RdpFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Default for RdpFrame {
    fn default() -> Self {
        Self { width: 0, height: 0, data: Vec::new() }
    }
}

/// RDP input event variants.
#[derive(Debug, Clone)]
pub enum RdpInput {
    Keyboard { scancode: u16, down: bool },
    Mouse { flags: u16, x: u16, y: u16 },
}

/// Errors produced during RDP connection / polling.
#[derive(Debug, Clone)]
pub enum RdpError {
    ConnectFailed(String),
    Disconnected,
    TlsError,
    Other(String),
}

impl fmt::Display for RdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RdpError::ConnectFailed(msg) => write!(f, "RDP connect failed: {msg}"),
            RdpError::Disconnected => write!(f, "RDP disconnected"),
            RdpError::TlsError => write!(f, "RDP TLS error"),
            RdpError::Other(msg) => write!(f, "RDP error: {msg}"),
        }
    }
}

impl std::error::Error for RdpError {}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl RdpConnection {
    #[cfg(not(feature = "rdp"))]
    pub fn connect(
        _host: &str, _port: u16, _user: &str, _password: &str,
        _width: u32, _height: u32,
    ) -> Result<Self, RdpError> {
        Err(RdpError::Other(
            "RDP support not compiled (feature = \"rdp\" disabled)".into(),
        ))
    }

    #[cfg(feature = "rdp")]
    pub fn connect(
        host: &str, port: u16, user: &str, password: &str,
        width: u32, height: u32,
    ) -> Result<Self, RdpError> {
        let c_host = CString::new(host).map_err(|_| RdpError::Other("invalid host".into()))?;
        let c_user = CString::new(user).map_err(|_| RdpError::Other("invalid user".into()))?;
        let c_pass =
            CString::new(password).map_err(|_| RdpError::Other("invalid password".into()))?;
        let ptr = unsafe {
            rdp_shim_connect(c_host.as_ptr(), port, c_user.as_ptr(), c_pass.as_ptr(), width, height)
        };
        match NonNull::new(ptr) {
            Some(inner) => Ok(Self { inner }),
            None => Err(RdpError::ConnectFailed("rdp_shim_connect returned NULL".into())),
        }
    }

    #[cfg(not(feature = "rdp"))]
    pub fn poll(&self) -> Result<bool, RdpError> {
        Err(RdpError::Other("RDP support not compiled".into()))
    }

    #[cfg(feature = "rdp")]
    pub fn poll(&self) -> Result<bool, RdpError> {
        let alive = unsafe { rdp_shim_poll(self.inner.as_ptr()) };
        Ok(alive)
    }

    #[cfg(not(feature = "rdp"))]
    pub fn framebuffer(&self) -> Option<RdpFrame> { None }

    #[cfg(feature = "rdp")]
    pub fn framebuffer(&self) -> Option<RdpFrame> {
        let mut w: c_uint = 0;
        let mut h: c_uint = 0;
        let ptr = unsafe { rdp_shim_framebuffer(self.inner.as_ptr(), &mut w, &mut h) };
        if ptr.is_null() || w == 0 || h == 0 {
            return None;
        }
        let len = (w as usize) * (h as usize) * 4;
        let data = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        Some(RdpFrame { width: w, height: h, data })
    }

    #[cfg(not(feature = "rdp"))]
    pub fn send_keyboard(&self, _scancode: u16, _down: bool) {}

    #[cfg(feature = "rdp")]
    pub fn send_keyboard(&self, scancode: u16, down: bool) {
        unsafe { rdp_shim_send_keyboard(self.inner.as_ptr(), scancode, down) }
    }

    #[cfg(not(feature = "rdp"))]
    pub fn send_mouse(&self, _flags: u16, _x: u16, _y: u16) {}

    #[cfg(feature = "rdp")]
    pub fn send_mouse(&self, flags: u16, x: u16, y: u16) {
        unsafe { rdp_shim_send_mouse(self.inner.as_ptr(), flags, x, y) }
    }

    #[cfg(not(feature = "rdp"))]
    pub fn resize(&self, _width: u32, _height: u32) {}

    #[cfg(feature = "rdp")]
    pub fn resize(&self, width: u32, height: u32) {
        unsafe { rdp_shim_resize(self.inner.as_ptr(), width, height) }
    }
}

#[cfg(feature = "rdp")]
impl Drop for RdpConnection {
    fn drop(&mut self) {
        unsafe { rdp_shim_disconnect(self.inner.as_ptr()) }
    }
}

#[cfg(not(feature = "rdp"))]
impl Drop for RdpConnection {
    fn drop(&mut self) {}
}

// ---------------------------------------------------------------------------
// FreeRDP mouse event flags (from freerdp/input.h)
// ---------------------------------------------------------------------------

pub mod ptr_flags {
    pub const MOVE: u16 = 0x0800;
    pub const DOWN: u16 = 0x8000;
    pub const BUTTON1: u16 = 0x1000;
    pub const BUTTON2: u16 = 0x2000;
    pub const BUTTON3: u16 = 0x4000;
    pub const WHEEL: u16 = 0x0400;
    pub const WHEEL_NEGATIVE: u16 = 0x0080;
    pub const WHEEL_POSITIVE: u16 = 0x0000;
    pub const SCROLL_UP: u16 = WHEEL | 0x0078;
    pub const SCROLL_DOWN: u16 = WHEEL | WHEEL_NEGATIVE | 0x0078;
}

// ---------------------------------------------------------------------------
// Input helpers
// ---------------------------------------------------------------------------

/// PC/AT scancode set 1 lookup for common keys.
pub fn key_to_scancode(key: &str, ctrl: bool, alt: bool, shift: bool) -> Option<(u16, bool)> {
    let _ = (ctrl, alt, shift);
    match key {
        "a"|"A" => Some((0x1E, false)), "b"|"B" => Some((0x30, false)),
        "c"|"C" => Some((0x2E, false)), "d"|"D" => Some((0x20, false)),
        "e"|"E" => Some((0x12, false)), "f"|"F" => Some((0x21, false)),
        "g"|"G" => Some((0x22, false)), "h"|"H" => Some((0x23, false)),
        "i"|"I" => Some((0x17, false)), "j"|"J" => Some((0x24, false)),
        "k"|"K" => Some((0x25, false)), "l"|"L" => Some((0x26, false)),
        "m"|"M" => Some((0x32, false)), "n"|"N" => Some((0x31, false)),
        "o"|"O" => Some((0x18, false)), "p"|"P" => Some((0x19, false)),
        "q"|"Q" => Some((0x10, false)), "r"|"R" => Some((0x13, false)),
        "s"|"S" => Some((0x1F, false)), "t"|"T" => Some((0x14, false)),
        "u"|"U" => Some((0x16, false)), "v"|"V" => Some((0x2F, false)),
        "w"|"W" => Some((0x11, false)), "x"|"X" => Some((0x2D, false)),
        "y"|"Y" => Some((0x15, false)), "z"|"Z" => Some((0x2C, false)),
        "0" => Some((0x0B, false)), "1" => Some((0x02, false)),
        "2" => Some((0x03, false)), "3" => Some((0x04, false)),
        "4" => Some((0x05, false)), "5" => Some((0x06, false)),
        "6" => Some((0x07, false)), "7" => Some((0x08, false)),
        "8" => Some((0x09, false)), "9" => Some((0x0A, false)),
        "-"|"_" => Some((0x0C, false)), "="|"+" => Some((0x0D, false)),
        "["|"{" => Some((0x1A, false)), "]"|"}" => Some((0x1B, false)),
        ";"|":" => Some((0x27, false)), "'"|"\"" => Some((0x28, false)),
        ""|"~" => Some((0x29, false)), "\\"|"|" => Some((0x2B, false)),
        ","|"<" => Some((0x33, false)), "."|">" => Some((0x34, false)),
        "/"|"?" => Some((0x35, false)),
        "\r"|"\n" => Some((0x1C, false)), "\t" => Some((0x0F, false)),
        "\x1b" => Some((0x01, false)), " " => Some((0x39, false)),
        "\x08"|"\x7f" => Some((0x0E, false)),
        "escape"|"tab"|"backspace"|"enter"|"return"|"space" => {
            match key {
                "escape" => Some((0x01, false)), "tab" => Some((0x0F, false)),
                "backspace" => Some((0x0E, false)),
                "enter"|"return" => Some((0x1C, false)),
                "space" => Some((0x39, false)),
                _ => unreachable!(),
            }
        },
        "up" => Some((0x48, true)), "down" => Some((0x50, true)),
        "left" => Some((0x4B, true)), "right" => Some((0x4D, true)),
        "home" => Some((0x47, true)), "end" => Some((0x4F, true)),
        "pageup" => Some((0x49, true)), "pagedown" => Some((0x51, true)),
        "insert" => Some((0x52, true)), "delete" => Some((0x53, true)),
        "f1" => Some((0x3B, false)), "f2" => Some((0x3C, false)),
        "f3" => Some((0x3D, false)), "f4" => Some((0x3E, false)),
        "f5" => Some((0x3F, false)), "f6" => Some((0x40, false)),
        "f7" => Some((0x41, false)), "f8" => Some((0x42, false)),
        "f9" => Some((0x43, false)), "f10" => Some((0x44, false)),
        "f11" => Some((0x57, true)), "f12" => Some((0x58, true)),
        "shift"|"lshift" => Some((0x2A, false)),
        "rshift" => Some((0x36, false)),
        "ctrl"|"lctrl" => Some((0x1D, false)),
        "rctrl" => Some((0x1D, true)),
        "alt"|"lalt" => Some((0x38, false)),
        "ralt" => Some((0x38, true)),
        "super"|"meta"|"lmeta" => Some((0x5B, true)),
        "rmeta" => Some((0x5C, true)),
        "menu" => Some((0x5D, true)),
        "numpad-0" => Some((0x52, false)),
        "numpad-1" => Some((0x4F, false)),
        "numpad-2" => Some((0x50, false)),
        "numpad-3" => Some((0x51, false)),
        "numpad-4" => Some((0x4B, false)),
        "numpad-5" => Some((0x4C, false)),
        "numpad-6" => Some((0x4D, false)),
        "numpad-7" => Some((0x47, false)),
        "numpad-8" => Some((0x48, false)),
        "numpad-9" => Some((0x49, false)),
        "numpad-decimal" => Some((0x53, false)),
        "numpad-add" => Some((0x4E, false)),
        "numpad-subtract" => Some((0x4A, false)),
        _ => None,
    }
}
