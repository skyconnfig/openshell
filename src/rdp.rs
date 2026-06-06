//! RDP session manager.
//!
//! Each open RDP tab maps to exactly one RdpWorker. The worker runs on
//! a blocking thread (FreeRDP's event loop is synchronous); commands come
//! in via an MPSC channel and screen frames are pushed back via
//! UnboundedSender<RdpEvent>.
//!
//! When the `rdp` feature is disabled, stub types are provided.


use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::config::RdpSession;
#[cfg(feature = "rdp")]
use crate::rdp_ffi::{self, RdpConnection, RdpError};
pub use crate::rdp_ffi::RdpFrame;
pub use crate::rdp_ffi::RdpInput;

/// Handle retained by the UI layer to drive a running RDP session.
pub struct RdpHandle {
    pub tab_id: String,
    pub commands: UnboundedSender<RdpCommand>,
    #[allow(dead_code)]
    pub join: JoinHandle<()>,
}

impl RdpHandle {
    pub fn send_input(&self, input: RdpInput) {
        let _ = self.commands.send(RdpCommand::SendInput(input));
    }

    pub fn resize(&self, width: u32, height: u32) {
        let _ = self.commands.send(RdpCommand::Resize(width, height));
    }

    pub fn close(&self) {
        let _ = self.commands.send(RdpCommand::Close);
    }
}

/// Commands sent to the RDP worker from the UI.
#[derive(Debug)]
pub enum RdpCommand {
    SendInput(RdpInput),
    Resize(u32, u32),
    Close,
}

/// Events emitted back to the UI.
#[derive(Debug, Clone)]
pub enum RdpEvent {
    Connected,
    Closed(String),
    /// Latest screen framebuffer.
    Frame(RdpFrame),
    Status(String),
}

/// Spawn an RDP worker on the Tokio runtime.
///
/// Returns a handle for sending commands + a receiver for draining events
/// (typically on the Slint event loop).
pub fn spawn_rdp(
    runtime: &tokio::runtime::Handle,
    tab_id: String,
    session: RdpSession,
) -> (RdpHandle, UnboundedReceiver<RdpEvent>) {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<RdpCommand>();
    let (evt_tx, evt_rx) = mpsc::unbounded_channel::<RdpEvent>();

    // When the rdp feature is disabled, immediately send a closed event.
    #[cfg(not(feature = "rdp"))]
    {
        let _ = (runtime, &session);
        let _ = evt_tx.send(RdpEvent::Closed(
            "RDP support not compiled (feature = \"rdp\" disabled)".into(),
        ));
    }

    #[cfg(feature = "rdp")]
    let join = runtime.spawn_blocking(move || {
        if let Err(err) = run_rdp(session, cmd_rx, evt_tx.clone()) {
            let _ = evt_tx.send(RdpEvent::Closed(format!("{err:#}")));
        }
    });

    #[cfg(not(feature = "rdp"))]
    let join = runtime.spawn_blocking(move || {
        let _ = (cmd_rx, session);
    });

    (
        RdpHandle {
            tab_id,
            commands: cmd_tx,
            join,
        },
        evt_rx,
    )
}

/// Blocking RDP session loop.  Runs on a dedicated OS thread via
/// spawn_blocking.
#[cfg(feature = "rdp")]
fn run_rdp(
    session: RdpSession,
    mut commands: UnboundedReceiver<RdpCommand>,
    events: UnboundedSender<RdpEvent>,
) -> Result<(), RdpError> {
    let host = session.host.clone();
    let port = session.port;
    let user = session.username.clone();
    let password = session.password.as_str().to_string();
    let width = session.resolution_width;
    let height = session.resolution_height;

    let _ = events.send(RdpEvent::Status(format!(
        "Connecting to {}:{} ...",
        host, port
    )));

    // Try to connect.
    let conn = RdpConnection::connect(&host, port, &user, &password, width, height)?;
    let _ = events.send(RdpEvent::Connected);
    let _ = events.send(RdpEvent::Status(format!("Connected to {}:{}", host, port)));

    // Track the last frame sent to avoid spamming identical frames.
    let mut last_frame: Option<RdpFrame> = None;

    // Main pump loop.
    loop {
        // Poll the RDP event loop (non-blocking within the C shim).
        match conn.poll() {
            Ok(true) => {}
            Ok(false) => {
                let _ = events.send(RdpEvent::Closed("Remote disconnected".into()));
                break;
            }
            Err(e) => {
                let _ = events.send(RdpEvent::Closed(format!("{e}")));
                break;
            }
        }

        // Check for new framebuffer.
        if let Some(frame) = conn.framebuffer() {
            // Only send if the frame actually changed.
            let is_new = last_frame
                .as_ref()
                .map(|prev| prev.data != frame.data)
                .unwrap_or(true);
            if is_new {
                last_frame = Some(frame.clone());
                let _ = events.send(RdpEvent::Frame(frame));
            }
        }

        // Drain commands (non-blocking).
        loop {
            match commands.try_recv() {
                Ok(RdpCommand::SendInput(input)) => {
                    match input {
                        RdpInput::Keyboard { scancode, down } => {
                            conn.send_keyboard(scancode, down);
                        }
                        RdpInput::Mouse { flags, x, y } => {
                            conn.send_mouse(flags, x, y);
                        }
                    }
                }
                Ok(RdpCommand::Resize(w, h)) => {
                    conn.resize(w, h);
                }
                Ok(RdpCommand::Close) | Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = events.send(RdpEvent::Closed("Closed by user".into()));
                    return Ok(());
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }

        // Brief sleep to keep CPU usage sane while polling at high frequency.
        std::thread::sleep(Duration::from_millis(10));
    }

    Ok(())
}
