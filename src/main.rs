use eframe::egui;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread::{self, JoinHandle};
use std::env; // Added for Wayland detection
use std::net::TcpStream; // Added for TCP connection

// Import rdev types needed for the listener functionality
// (These were previously commented out at the bottom or in a different scope)
use rdev::{listen, Event as RdevEvent, EventType as RdevEventType, Key as RdevKey, Button as RdevButton};
use serde::{Serialize, Deserialize};
use local_ip_address::local_ip; // Added for local IP

// --- Network Message Definitions ---
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

#[derive(Serialize, Deserialize, Debug, Clone)] pub enum SerializableRdevEventType {
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
                // RdevEventType::Unknown is not a variant with {code, event_type}. 
                // We catch all other rdev event types here.
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
enum RdevThreadMessage {
    Event(RdevEvent),
    Error(String),
    Started { width: u32, height: u32 },
    Stopped,
}

// Helper function to check for Wayland environment
fn is_running_on_wayland() -> bool {
    env::var("WAYLAND_DISPLAY").is_ok()
}

// --- New messages for Connection Thread ---
#[derive(Debug)]
enum ConnectionThreadMessage {
    Connected(TcpStream),
    ConnectionFailed(String),
    Disconnected,
}
// --- End Connection Thread Messages ---

struct MyApp {
    value: i32, // Placeholder
    macos_accessibility_granted: Option<bool>,

    rdev_listener_handle: Option<JoinHandle<()>>,
    rdev_thread_tx: Option<Sender<RdevThreadMessage>>, // To send control messages TO the rdev thread (e.g., shutdown)
    rdev_gui_rx: Option<Receiver<RdevThreadMessage>>,   // To receive messages FROM the rdev thread in the GUI
    listener_status: String,
    last_event_summary: String, // To display a summary of the last received event

    // New fields for screen edge detection
    screen_width: u32,
    screen_height: u32,
    current_mouse_x: f64,
    current_mouse_y: f64,
    edge_status: String,

    // New fields for networking placeholder
    my_local_ip: String, // To display own IP
    peer_address_input: String, // For IP or future Peer ID
    connection_status: String, // e.g., Disconnected, Connecting, Connected to <Peer>
    is_connected: bool, // Simple flag for now

    // New fields for TCP connection management
    tcp_stream: Option<TcpStream>,
    connection_thread_handle: Option<JoinHandle<()>>,
    // No specific sender needed TO the connection thread yet, it's fire-and-forget to connect.
    connection_gui_rx: Option<Receiver<ConnectionThreadMessage>>,
}

impl Default for MyApp {
    fn default() -> Self {
        let local_ip_string = match local_ip() {
            Ok(ip) => ip.to_string(),
            Err(e) => format!("Error getting local IP: {}", e),
        };

        Self {
            value: 0,
            macos_accessibility_granted: None,
            rdev_listener_handle: None,
            rdev_thread_tx: None,
            rdev_gui_rx: None,
            listener_status: "Listener not started.".to_string(),
            last_event_summary: "No events received yet.".to_string(),
            screen_width: 0, // Will be updated when listener starts
            screen_height: 0,
            current_mouse_x: 0.0,
            current_mouse_y: 0.0,
            edge_status: "Mouse not at edge".to_string(),
            my_local_ip: local_ip_string, // Initialize with local IP
            peer_address_input: String::new(),
            connection_status: "Disconnected".to_string(),
            is_connected: false,
            tcp_stream: None,
            connection_thread_handle: None,
            connection_gui_rx: None,
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for messages from the rdev thread on each UI update
        if let Some(rx) = &self.rdev_gui_rx {
            // Use try_recv for non-blocking check
            while let Ok(message) = rx.try_recv() {
                match message {
                    RdevThreadMessage::Event(event) => {
                        // Convert to serializable event for potential network sending
                        let _serializable_event = SerializableRdevEvent::from(event.clone());
                        // For now, just update local summary and mouse state
                        let mut summary = format!("Event: {:?}", event.event_type);
                        if let Some(name) = event.name {
                            summary.push_str(&format!(" ({})", name));
                        }
                        self.last_event_summary = summary;

                        if let RdevEventType::MouseMove { x, y } = event.event_type {
                            self.current_mouse_x = x;
                            self.current_mouse_y = y;

                            // Check for screen edge
                            if self.screen_width > 0 && self.screen_height > 0 { // Ensure screen dimensions are known
                                let at_left = x <= 1.0;
                                let at_right = x >= (self.screen_width as f64 - 2.0); // -1 or -2 to be safe
                                let at_top = y <= 1.0;
                                let at_bottom = y >= (self.screen_height as f64 - 2.0);

                                if at_left {
                                    self.edge_status = "Mouse at LEFT edge".to_string();
                                } else if at_right {
                                    self.edge_status = "Mouse at RIGHT edge".to_string();
                                } else if at_top {
                                    self.edge_status = "Mouse at TOP edge".to_string();
                                } else if at_bottom {
                                    self.edge_status = "Mouse at BOTTOM edge".to_string();
                                } else {
                                    self.edge_status = "Mouse not at edge".to_string();
                                }
                            }
                        }
                        // TODO: If self.is_connected and edge is hit, attempt to send _serializable_event
                        // to the connected peer (conceptual for now).
                        // if self.is_connected && (self.edge_status.contains("LEFT") || self.edge_status.contains("RIGHT") || /* ... */) {
                        //     println!("NETWORK_TODO: Send {:?} to peer", _serializable_event);
                        // }
                    }
                    RdevThreadMessage::Error(err_msg) => {
                        self.listener_status = format!("Error: {}", err_msg);
                        self.last_event_summary = "An error occurred.".to_string();
                        // Ensure we clean up if the thread stops due to an error
                        if self.rdev_listener_handle.is_some() {
                            self.rdev_listener_handle.take().unwrap().join().ok(); // Join the thread
                        }
                        self.rdev_thread_tx = None; // Clear sender as thread is gone
                        // GUI receiver (self.rdev_gui_rx) is left to drain any pending messages
                    }
                    RdevThreadMessage::Started { width, height } => {
                        self.listener_status = "Listener active.".to_string();
                        // Get screen dimensions when listener starts
                        self.screen_width = width;
                        self.screen_height = height;
                        self.last_event_summary = format!("Screen: {}x{}", width, height); // Update summary
                    }
                    RdevThreadMessage::Stopped => {
                        self.listener_status = "Listener stopped.".to_string();
                        if let Some(handle) = self.rdev_listener_handle.take() {
                            handle.join().ok(); // Join the thread
                        }
                        self.rdev_thread_tx = None; // Clear sender as thread is likely gone or stopping
                    }
                }
            }
        }

        // Check for messages from the Connection thread
        if let Some(rx) = &self.connection_gui_rx {
            while let Ok(message) = rx.try_recv() {
                match message {
                    ConnectionThreadMessage::Connected(stream) => {
                        self.tcp_stream = Some(stream);
                        self.is_connected = true;
                        self.connection_status = format!("Successfully connected to {}", self.peer_address_input);
                        println!("TCP Connection established with: {}", self.peer_address_input);
                        if let Some(handle) = self.connection_thread_handle.take() {
                            handle.join().expect("Connection thread failed to join");
                        }
                    }
                    ConnectionThreadMessage::ConnectionFailed(err_msg) => {
                        self.is_connected = false;
                        self.connection_status = format!("Connection failed: {}", err_msg);
                        eprintln!("TCP Connection failed: {}", err_msg);
                        if let Some(handle) = self.connection_thread_handle.take() {
                            handle.join().expect("Connection thread failed to join (after error)");
                        }
                    }
                    ConnectionThreadMessage::Disconnected => {
                        self.is_connected = false;
                        self.tcp_stream = None; // Ensure stream is cleared
                        self.connection_status = "Disconnected by peer or error during operation.".to_string();
                        println!("TCP Disconnected (message from hypothetical read/write thread).");
                        // In future, if a read/write thread signals this, we might also join its handle here.
                    }
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Rust KVM Experiment");
            ui.horizontal(|ui| {
                ui.label("Placeholder value:");
                ui.add(egui::DragValue::new(&mut self.value));
            });

            ui.separator();

            // macOS Accessibility Check
            if cfg!(target_os = "macos") {
                if self.macos_accessibility_granted.is_none() {
                    // Attempt a check if unknown.
                    // For a real check, we'd try a minimal rdev operation here
                    // or use a specific macOS API if available.
                    // This is a placeholder for the actual check logic.
                    // Let\'s simulate a check for now.
                    // In a real scenario, this might involve trying to rdev::listen
                    // in a non-blocking way or using another method.
                    // For now, we'll assume we need to guide the user if it\'s the first run or unknown.
                    ui.label("Checking macOS Accessibility permissions...");
                    // Placeholder: In a real app, you'd call a function here that tries
                    // to use rdev or a macOS API to determine status.
                    // For this example, let\'s just pretend it becomes false if not yet set.
                    // self.macos_accessibility_granted = Some(check_macos_accessibility());
                    // We'll manually set it for demonstration purposes in a real scenario.
                    // For now, let\'s just show the message.
                }

                match self.macos_accessibility_granted {
                    Some(true) => {
                        ui.colored_label(egui::Color32::GREEN, "macOS Accessibility permissions appear to be granted.");
                    }
                    Some(false) => {
                        ui.colored_label(egui::Color32::RED, "macOS Accessibility permissions are NOT granted.");
                        ui.label("To use this application fully, please grant Accessibility access.");
                        ui.label("Go to System Settings > Privacy & Security > Accessibility.");
                        ui.label("Click the '+' button, find this application (or your terminal if running via 'cargo run'), and enable it.");
                        if ui.button("Open Accessibility Settings").clicked() {
                            // Try to open the settings pane
                            // This is platform-specific and might need a crate or direct OS calls.
                            // For macOS:
                            let _ = std::process::Command::new("open")
                                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                                .status();
                        }
                    }
                    None => {
                         // Still unknown, show a button to manually trigger a "check" or guide
                        ui.label("macOS Accessibility permission status is unknown.");
                        ui.label("Please ensure permissions are granted as per instructions above.");
                         if ui.button("Re-check Accessibility (Simulated - Set to Denied for Demo)").clicked() {
                            self.macos_accessibility_granted = Some(false); // Simulate finding it's denied
                        }
                        if ui.button("Simulate Permission Granted").clicked() {
                            self.macos_accessibility_granted = Some(true);
                        }
                    }
                }
            } else {
                ui.label("Not on macOS, no specific accessibility permission check needed here.");
            }

            ui.separator();
            ui.heading("Event Listener Control");

            if self.rdev_listener_handle.is_none() { // If listener is not running
                if ui.button("Start Listener").clicked() {
                    let (gui_tx, gui_rx) = channel::<RdevThreadMessage>();
                    self.rdev_gui_rx = Some(gui_rx);
                    
                    let thread_gui_tx_for_spawn = gui_tx.clone();
                    let use_grab = is_running_on_wayland();

                    if use_grab {
                        self.listener_status = "Starting listener (Wayland mode using grab)...".to_string();
                        println!("Attempting to start rdev listener in Wayland (grab) mode.");
                    } else {
                        self.listener_status = "Starting listener (X11 mode using listen)...".to_string();
                        println!("Attempting to start rdev listener in X11 (listen) mode.");
                    }
                    self.last_event_summary = "Waiting for events...".to_string();

                    let handle = thread::spawn(move || {
                        let event_tx = thread_gui_tx_for_spawn.clone(); // For events
                        let status_tx = thread_gui_tx_for_spawn;      // For Started and Error/Stopped

                        // Get display size
                        let (screen_width, screen_height) = match rdev::display_size() {
                            Ok((w, h)) => (w, h),
                            Err(e) => {
                                let err_msg = format!("Failed to get display size: {:?}. Edge detection will be unreliable.", e);
                                eprintln!("{}", err_msg);
                                // Send an error, but we might still try to start the listener without screen info
                                // or with (0,0) which would make edge detection not work.
                                // For now, let's send the error and stop the thread to make it clear.
                                status_tx.send(RdevThreadMessage::Error(err_msg)).unwrap_or_default();
                                return;
                            }
                        };
                        // Send Started message with actual dimensions
                        status_tx.send(RdevThreadMessage::Started { width: screen_width as u32, height: screen_height as u32 }).unwrap_or_default();

                        if use_grab {
                            // Wayland: Use rdev::grab()
                            println!("rdev listener thread: Using GRAB (Wayland mode).");
                            let grab_callback = move |event: RdevEvent| -> Option<RdevEvent> {
                                if event_tx.send(RdevThreadMessage::Event(event.clone())).is_err() {
                                    println!("rdev (grab): GUI channel closed, can't send event. Listener continues.");
                                    // To stop grab, the thread would need a signal to terminate itself.
                                    // For now, if channel is closed, events are just dropped by the send.
                                }
                                Some(event) // Pass the event through
                            };
                            if let Err(error) = rdev::grab(grab_callback) {
                                let error_msg = format!(
                                    "Wayland Listener (grab) error: {:?}.\n\nThis can happen if an input device (e.g., /dev/input/eventX) has restrictive permissions that prevent access even if you are in the 'input' group. Some systems have special hardware keys (like vendor-specific function keys) that remain root-only.\n\nEnsure you are a member of the 'input' group (or 'plugdev' on some systems) and have restarted your session. If the issue persists, a specific input device might be the cause. Running 'ls -l /dev/input/' might help identify such devices (look for ones not belonging to the 'input' group or with restrictive ACLs).",
                                    error
                                );
                                eprintln!("rdev grab error: {}", error_msg);
                                status_tx.send(RdevThreadMessage::Error(error_msg)).unwrap_or_default();
                            } else {
                                println!("rdev grab finished without an explicit error.");
                                status_tx.send(RdevThreadMessage::Stopped).unwrap_or_default();
                            }
                        } else {
                            // X11: Use rdev::listen()
                            println!("rdev listener thread: Using LISTEN (X11 mode).");
                            if let Err(error) = rdev::listen(move |event| {
                                if event_tx.send(RdevThreadMessage::Event(event)).is_err() {
                                    println!("rdev (listen): GUI channel closed, can't send event. Listener continues.");
                                }
                            }) {
                                let error_msg = format!("X11 Listener (listen) error: {:?}", error);
                                eprintln!("rdev listen error: {}", error_msg);
                                status_tx.send(RdevThreadMessage::Error(error_msg)).unwrap_or_default();
                            } else {
                                println!("rdev listen finished without an explicit error.");
                                status_tx.send(RdevThreadMessage::Stopped).unwrap_or_default();
                            }
                        }
                    });
                    self.rdev_listener_handle = Some(handle);
                }
            } else { // If listener IS running
                if ui.button("Stop Listener").clicked() {
                    // Stopping a blocking rdev::listen is tricky.
                    // rdev doesn't offer a direct API to stop listen().
                    // The most straightforward way with current rdev is to drop the JoinHandle, which detaches the thread.
                    // The thread will continue running until rdev::listen errors out or the program exits.
                    // For a cleaner shutdown, the rdev thread would need to periodically check a flag 
                    // (e.g., via another channel `rdev_control_rx`)
                    // but rdev::listen itself is a blocking call, so it can't check a flag while blocked.
                    // So, for now, "Stop" is more like "Abandon listener thread and hope it exits on program close".
                    // A more robust solution would involve platform-specific thread interruption or a modified rdev.
                    
                    println!("Stop Listener clicked. Attempting to signal thread (currently not implemented for blocking rdev::listen).");
                    self.listener_status = "Stop request sent (actual stop depends on thread completion).".to_string();
                    
                    // If we had a control channel to the rdev thread:
                    // if let Some(tx) = &self.rdev_thread_tx {
                    //     tx.send(()).unwrap_or_default(); // Send a shutdown signal
                    // }

                    // For now, we can only really stop listening for *new* messages and take the handle.
                    // The thread itself will run until rdev::listen finishes (likely on error or app exit).
                    if let Some(handle) = self.rdev_listener_handle.take() {
                        // We could try to join with a timeout, but a blocking listen() won't respect it.
                        // For now, we just take the handle. The thread keeps running.
                        // handle.join().ok(); // This would block the GUI if the thread doesn't exit.
                        println!("Listener thread handle taken. GUI will no longer process its messages after this update loop.");
                    }
                    // Clear the sender to the rdev thread as we are "stopping" it.
                    self.rdev_thread_tx = None; 
                    // We keep rdev_gui_rx to drain any final messages, but then it should ideally be cleared or handled.
                    // self.rdev_gui_rx = None; // Or handle this more gracefully.
                    
                    // Simulate a stopped status for the UI, though the thread might still be technically running.
                    // The next time update runs and if the thread did stop and sent a message, it will update.
                    self.listener_status = "Listener abandoned by GUI. May still run until app exit.".to_string();
                }
            }

            ui.separator();
            ui.label(format!("Listener Status: {}", self.listener_status));
            ui.label(format!("Last Event: {}", self.last_event_summary));

            ui.separator();
            ui.heading("Mouse & Screen Info");
            ui.label(format!("Screen Dimensions: {}x{}", self.screen_width, self.screen_height));
            ui.label(format!("Mouse Position: ({:.0}, {:.0})", self.current_mouse_x, self.current_mouse_y));
            ui.label(format!("Edge Status: {}", self.edge_status));

            // --- Networking UI Placeholder ---
            ui.separator();
            ui.heading("Networking");

            ui.label("My Connection Details:");
            ui.horizontal(|ui| {
                ui.label("Local IP Address:");
                // Make the IP address selectable so the user can copy it
                let mut ip_to_show = self.my_local_ip.clone();
                ui.add(egui::TextEdit::singleline(&mut ip_to_show).interactive(false));
            });
            
            ui.separator();
            ui.label("Connect to Peer:");
            ui.horizontal(|ui| {
                ui.label("Peer Address/ID (e.g., 127.0.0.1:7878):");
                ui.add_enabled(!self.is_connected && self.connection_thread_handle.is_none(), 
                    egui::TextEdit::singleline(&mut self.peer_address_input)
                );
            });

            if self.connection_thread_handle.is_some() {
                ui.label("Processing connection attempt...");
            } else if !self.is_connected {
                if ui.button("Connect").clicked() {
                    if !self.peer_address_input.is_empty() {
                        self.connection_status = format!("Attempting to connect to {}...", self.peer_address_input);
                        let (conn_tx, conn_rx) = channel::<ConnectionThreadMessage>();
                        self.connection_gui_rx = Some(conn_rx);
                        let peer_addr_clone = self.peer_address_input.clone();
                        
                        let handle = thread::spawn(move || {
                            let mut addr_to_connect = peer_addr_clone.clone(); // Clone again for modification
                            if !addr_to_connect.contains(':') {
                                let default_port = 7878;
                                println!("Port not specified for {}, appending default port {}.", addr_to_connect, default_port);
                                addr_to_connect = format!("{}:{}", addr_to_connect, default_port);
                            }

                            println!("Connection thread: Attempting to connect to {}", addr_to_connect);
                            match TcpStream::connect(&addr_to_connect) {
                                Ok(stream) => {
                                    println!("Connection thread: Successfully connected to {}", addr_to_connect);
                                    conn_tx.send(ConnectionThreadMessage::Connected(stream)).unwrap_or_default();
                                }
                                Err(e) => {
                                    eprintln!("Connection thread: Failed to connect to {}: {}", addr_to_connect, e);
                                    conn_tx.send(ConnectionThreadMessage::ConnectionFailed(e.to_string())).unwrap_or_default();
                                }
                            }
                        });
                        self.connection_thread_handle = Some(handle);
                    } else {
                        self.connection_status = "Error: Peer address is empty.".to_string();
                    }
                }
            } else { // is_connected
                if ui.button("Disconnect").clicked() {
                    if let Some(stream) = self.tcp_stream.take() {
                        println!("Disconnecting from peer...");
                        if let Err(e) = stream.shutdown(std::net::Shutdown::Both) {
                            eprintln!("Error shutting down TCP stream: {}", e);
                        }
                        // The stream is dropped here, closing the connection.
                    }
                    self.is_connected = false;
                    self.connection_status = "Disconnected".to_string();
                    // self.peer_address_input.clear(); // Optionally clear input on disconnect
                    // If a connection thread was somehow still around (shouldn't be if connected), handle it.
                    if let Some(handle) = self.connection_thread_handle.take() {
                        handle.join().expect("Connection thread (during disconnect) failed to join");
                    }
                }
            }
            ui.label(format!("Connection Status: {}", self.connection_status));
            // --- End Networking UI Placeholder ---

        });

        // Request a repaint if the listener might be active, to process messages
        if self.rdev_listener_handle.is_some() || self.connection_thread_handle.is_some() {
            ctx.request_repaint();
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([600.0, 800.0]), // Increased height a bit
        ..Default::default()
    };
    eframe::run_native(
        "Rust KVM Experiment",
        options,
        Box::new(|_cc| Box::<MyApp>::default()),
    )
}

// Placeholder for actual permission check function
// fn check_macos_accessibility() -> bool {
//     // This is the tricky part.
//     // One way is to try a minimal rdev::listen or rdev::grab in a non-blocking
//     // or very short-lived thread and see if it errors out with a permission-denied error.
//     // rdev::listen itself is blocking. rdev::grab with a feature flag might be better.
//     //
//     // Example (conceptual, rdev::listen is blocking!):
//     // std::thread::spawn(|| {
//     //     if let Err(e) = rdev::listen(|_| {}) {
//     //         // Check error type for permission denied
//     //         // This is difficult because listen is blocking and designed to run for a long time.
//     //         // return false;
//     //     }
//     //     // return true; if it didn't error in a way that indicates denial
//     // }).join().unwrap_or(false)
//     //
//     // A better approach for macOS might be to use the `IOKit` or other system frameworks
//     // to check if the current process has the necessary entitlements/permissions,
//     // or if an event tap can be successfully created.
//     //
//     // For now, return false to show the message.
//     false
// }

// Previous rdev listener code (to be integrated later)
/*
use rdev::{listen, Event, EventType, Key, Button};

fn run_rdev_listener() {
    println!("Starting event listener (from GUI context)...");
    if let Err(error) = listen(rdev_callback) {
        println!("Error in rdev listener: {:?}", error);
    }
}

fn rdev_callback(event: Event) {
    // This callback will likely need to send events to the GUI thread
    // using something like an mpsc channel, or update shared state
    // if the GUI needs to react to these events.
    // For now, just print.
    match event.event_type {
        EventType::MouseMove { x, y } => {
            // println!("Mouse_Move_Detected");
        }
        EventType::KeyPress(key) => {
            println!("GUI: Key_Press: {:?}", key);
            if let Some(name) = event.name {
                println!("  GUI: Resolved_Name: {}", name);
            }
        }
        // ... other event types ...
        _ => println!("GUI: Other_Event: {:?}"),
    }
}
*/
