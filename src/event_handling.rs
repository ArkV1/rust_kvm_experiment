// Module for input event handling (rdev, screen info, mouse logic) 

use rdev::{Event as RdevEvent, EventType as RdevEventType, Key as RdevKey, Button as RdevButton};
use serde::{Serialize, Deserialize};
use std::env;
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};

// --- Network Message Definitions (subset relevant to events, or consider a shared module later) ---
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum NetworkKvmMessage {
    Heartbeat,
    Event(SerializableRdevEvent),
    // We might add other message types later, e.g., ClipboardData, ConfigSync
}

// rdev::Event itself is not directly Serialize/Deserialize friendly due to nested types
// that might not implement it or are platform specific in ways serde struggles with easily.
// We create a simplified, serializable version.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SerializableRdevEvent {
    // pub time: SystemTime, // SystemTime can be tricky to serialize consistently across platforms/serde formats.
                           // We might add a timestamp later if needed, perhaps as u64 millis.
    pub name: Option<String>, // Corresponds to rdev::Event.name
    pub event_type: SerializableRdevEventType, // Corresponds to rdev::Event.event_type
}

#[derive(Serialize, Deserialize, Debug, Clone)] 
pub enum SerializableRdevEventType {
    KeyPress(RdevKey),
    KeyRelease(RdevKey),
    ButtonPress(RdevButton),
    ButtonRelease(RdevButton),
    MouseMove { x: f64, y: f64 },
    Wheel { delta_x: i64, delta_y: i64 },
    Unknown { code: u32, event_type_code: u16 }, // Represents an event we couldn't specifically map
}

// Helper to convert from rdev::Event to SerializableRdevEvent
impl From<RdevEvent> for SerializableRdevEvent {
    fn from(event: RdevEvent) -> Self {
        SerializableRdevEvent {
            name: event.name,
            event_type: match event.event_type {
                RdevEventType::KeyPress(k) => SerializableRdevEventType::KeyPress(k),
                RdevEventType::KeyRelease(k) => SerializableRdevEventType::KeyRelease(k),
                RdevEventType::ButtonPress(b) => SerializableRdevEventType::ButtonPress(b),
                RdevEventType::ButtonRelease(b) => SerializableRdevEventType::ButtonRelease(b),
                RdevEventType::MouseMove { x, y } => SerializableRdevEventType::MouseMove { x, y },
                RdevEventType::Wheel { delta_x, delta_y } => SerializableRdevEventType::Wheel { delta_x, delta_y },
                other => {
                    println!("Warning: Unhandled rdev::EventType {:?}. Mapping to generic Unknown.", other);
                    SerializableRdevEventType::Unknown { code: 0, event_type_code: 0 } // Placeholder codes
                }
            },
        }
    }
}
// --- End Network Message Definitions ---

// Define a message type to send from the rdev thread to the GUI thread
#[derive(Debug)]
pub enum RdevThreadMessage { // Made public
    Event(RdevEvent),
    Error(String),
    Started { width: u32, height: u32 },
}

// Helper function to check for Wayland environment
pub fn is_running_on_wayland() -> bool { // Made public
    let wayland_display = env::var("WAYLAND_DISPLAY");
    let xdg_session_type = env::var("XDG_SESSION_TYPE");

    println!("DEBUG: WAYLAND_DISPLAY env var: {:?}", wayland_display);
    println!("DEBUG: XDG_SESSION_TYPE env var: {:?}", xdg_session_type);

    wayland_display.is_ok() || xdg_session_type.map_or(false, |s| s.eq_ignore_ascii_case("wayland"))
}

pub fn spawn_rdev_listener_thread(gui_tx: Sender<RdevThreadMessage>) -> JoinHandle<()> {
    let use_grab = is_running_on_wayland(); // is_running_on_wayland is already in this module

    thread::spawn(move || {
        let event_tx = gui_tx.clone();
        let status_tx = gui_tx;

        let (screen_width, screen_height) = match rdev::display_size() {
            Ok((w, h)) => (w, h),
            Err(e) => {
                let err_msg = format!("Failed to get display size: {:?}. Edge detection will be unreliable.", e);
                eprintln!("{}", err_msg);
                status_tx.send(RdevThreadMessage::Error(err_msg)).unwrap_or_default();
                return;
            }
        };
        status_tx.send(RdevThreadMessage::Started { width: screen_width as u32, height: screen_height as u32 }).unwrap_or_default();

        if use_grab {
            println!("rdev listener thread: Using GRAB (Wayland mode).");
            let grab_callback = move |event: RdevEvent| -> Option<RdevEvent> {
                if event_tx.send(RdevThreadMessage::Event(event.clone())).is_err() {
                    println!("rdev (grab): GUI channel closed, can't send event. Listener continues.");
                }
                Some(event) // Pass the event through
            };
            if let Err(error) = rdev::grab(grab_callback) {
                let error_msg = format!(
                    "Wayland Listener (grab) error: {:?}.\n\nTo fix this (on Linux), the process needs to run as a user who is a member of the 'input' group (recommended). On some distros, the group might be 'plugdev'. If in doubt, add your user to both groups if they exist and restart your session. Alternatively, running as root (not recommended) might work.",
                    error
                );
                eprintln!("rdev grab error: {}", error_msg);
                status_tx.send(RdevThreadMessage::Error(error_msg)).unwrap_or_default();
            } else {
                println!("rdev grab finished without an explicit error.");
            }
        } else {
            println!("rdev listener thread: Using LISTEN (X11 mode).");
            if let Err(error) = rdev::listen(move |event| {
                println!("RDEV_LISTEN_CALLBACK: {:?}", event.event_type);
                if event_tx.send(RdevThreadMessage::Event(event)).is_err() {
                    println!("rdev (listen): GUI channel closed, can't send event. Listener continues.");
                }
            }) {
                let error_msg = format!("X11 Listener (listen) error: {:?}", error);
                eprintln!("rdev listen error: {}", error_msg);
                status_tx.send(RdevThreadMessage::Error(error_msg)).unwrap_or_default();
            } else {
                println!("rdev listen finished without an explicit error (unexpected).");
            }
        }
    })
} 