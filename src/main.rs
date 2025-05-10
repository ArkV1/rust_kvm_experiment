use eframe::egui;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread::{self, JoinHandle};
use std::env; // Added for Wayland detection
use std::net::{TcpStream, TcpListener, SocketAddr, Shutdown}; // Added TcpListener, SocketAddr, and Shutdown
use std::io::Write; // Added for stream.write_all

// Import rdev types needed for the listener functionality
// (These were previously commented out at the bottom or in a different scope)
use rdev::{Event as RdevEvent, EventType as RdevEventType, Key as RdevKey, Button as RdevButton};
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
    // NOTE: Assuming 'Stopped' variant was intentionally removed in origin/master based on conflict pattern.
    // If 'Stopped' is needed, it should be re-added here and handled in the match below.
}

// Helper function to check for Wayland environment
fn is_running_on_wayland() -> bool {
    env::var("WAYLAND_DISPLAY").is_ok() || env::var("XDG_SESSION_TYPE").map_or(false, |s| s.eq_ignore_ascii_case("wayland"))
}

// --- New messages for Connection Thread ---
#[derive(Debug)]
enum ConnectionThreadMessage {
    Connected(TcpStream),
    ConnectionFailed(String),
    Disconnected,
}
// --- End Connection Thread Messages ---

// --- New messages for Server Listener Thread ---
#[derive(Debug)]
enum ServerThreadMessage {
    Listening(SocketAddr),
    NewConnection(TcpStream, SocketAddr), // Carries the new stream and the peer's address
    BindError(String),
    AcceptError(String),
    // We might add ListenerStopped if we make stop more graceful
}
// --- End Server Listener Thread Messages ---

struct MyApp {
    value: i32, // Placeholder
    macos_accessibility_granted: Option<bool>,

    rdev_listener_handle: Option<JoinHandle<()>>,
    rdev_thread_tx: Option<Sender<RdevThreadMessage>>, // To send control messages TO the rdev thread (e.g., shutdown)
    rdev_gui_rx: Option<Receiver<RdevThreadMessage>>,   // To receive messages FROM the rdev thread in the GUI
    input_listener_status: String, // Renamed from listener_status for clarity
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

    // New fields for TCP Listener (Server) functionality
    tcp_listener_handle: Option<JoinHandle<()>>, // Separate handle for the listener thread
    // tcp_listener: Option<TcpListener>, // The listener itself will live in its thread
    server_gui_rx: Option<Receiver<ServerThreadMessage>>, // Channel to get messages from server thread
    server_status: String, // e.g., "Not listening", "Listening on 0.0.0.0:7878"
    is_listening: bool,
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
            input_listener_status: "Input Listener not started.".to_string(),
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
            
            // New defaults for server functionality
            tcp_listener_handle: None,
            server_gui_rx: None,
            server_status: "Not listening.".to_string(),
            is_listening: false,
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for messages from the rdev thread
        if let Some(rx) = &self.rdev_gui_rx {
            while let Ok(message) = rx.try_recv() {
                match message {
                    RdevThreadMessage::Event(event) => {
                        let serializable_event = SerializableRdevEvent::from(event.clone());
                        let mut summary = format!("Event: {:?}", event.event_type);
                        if let Some(name) = event.name {
                            summary.push_str(&format!(" ({})", name));
                        }
                        self.last_event_summary = summary;

                        // --- Send event over TCP if connected --- 
                        if self.is_connected {
                            if let Some(stream) = &mut self.tcp_stream {
                                match bincode::serialize(&serializable_event) {
                                    Ok(encoded_event) => {
                                        let event_len = encoded_event.len() as u32;
                                        // Send the length of the event data as u32 first
                                        if let Err(e) = stream.write_all(&event_len.to_be_bytes()) {
                                            eprintln!("Failed to send event length: {}. Disconnecting.", e);
                                            self.is_connected = false;
                                            self.tcp_stream = None; // Clear the stream
                                            self.connection_status = "Disconnected (send error).".to_string();
                                        } else {
                                            // Then send the event data itself
                                            if let Err(e) = stream.write_all(&encoded_event) {
                                                eprintln!("Failed to send event data: {}. Disconnecting.", e);
                                                self.is_connected = false;
                                                self.tcp_stream = None; // Clear the stream
                                                self.connection_status = "Disconnected (send error).".to_string();
                                            }
                                            // Optionally: flush the stream if needed, though write_all often implies it for TCP.
                                            // if let Err(e) = stream.flush() { /* handle error */ }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to serialize event: {}", e);
                                        // Decide if this error should also cause disconnect or just log
                                    }
                                }
                            } else {
                                // This case (is_connected true but tcp_stream is None) should ideally not happen.
                                // If it does, it indicates a state inconsistency.
                                eprintln!("Error: is_connected is true, but no tcp_stream available to send event.");
                                self.is_connected = false; // Correct the state
                                self.connection_status = "Disconnected (internal error).".to_string();
                            }
                        }
                        // --- End send event over TCP ---

                        if let RdevEventType::MouseMove { x, y } = event.event_type {
                            self.current_mouse_x = x;
                            self.current_mouse_y = y;
                            if self.screen_width > 0 && self.screen_height > 0 { 
                                let at_left = x <= 1.0;
                                let at_right = x >= (self.screen_width as f64 - 2.0); 
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
                    }
                    RdevThreadMessage::Error(err_msg) => {
                        self.input_listener_status = format!("Input Listener Error: {}", err_msg);
                        self.last_event_summary = "An error occurred in input listener.".to_string();
                        if self.rdev_listener_handle.is_some() {
                            self.rdev_listener_handle.take().unwrap().join().ok(); 
                        }
                        self.rdev_thread_tx = None; 
                    }
                    RdevThreadMessage::Started { width, height } => {
                        self.input_listener_status = "Input Listener active.".to_string();
                        self.screen_width = width;
                        self.screen_height = height;
                        self.last_event_summary = format!("Screen: {}x{}", width, height); 
                    }
                }
            }
        }

        // Check for messages from the Connection thread
        if let Some(rx) = &self.connection_gui_rx {
            while let Ok(message) = rx.try_recv() {
                match message {
                    ConnectionThreadMessage::Connected(stream) => {
                        if self.is_connected {
                            println!("Already connected, ignoring new outgoing connection success for now.");
                        } else {
                            self.tcp_stream = Some(stream);
                            self.is_connected = true;
                            self.connection_status = format!("Successfully connected to {}", self.peer_address_input);
                            println!("TCP Connection established with: {}", self.peer_address_input);
                        }
                        if let Some(handle) = self.connection_thread_handle.take() {
                            handle.join().expect("Connection thread failed to join");
                        }
                    }
                    ConnectionThreadMessage::ConnectionFailed(err_msg) => {
                        self.is_connected = false; 
                        self.tcp_stream = None; 
                        self.connection_status = format!("Connection failed: {}", err_msg);
                        eprintln!("TCP Connection failed: {}", err_msg);
                        if let Some(handle) = self.connection_thread_handle.take() {
                            handle.join().expect("Connection thread failed to join (after error)");
                        }
                    }
                    ConnectionThreadMessage::Disconnected => { 
                        println!("Disconnected by peer (outgoing connection closed).");
                        self.is_connected = false;
                        self.tcp_stream = None; 
                        self.connection_status = "Disconnected by peer.".to_string();
                    }
                }
            }
        }

        // Check for messages from the Server Listener thread (incoming connections)
        if let Some(rx) = &self.server_gui_rx {
            while let Ok(message) = rx.try_recv() {
                match message {
                    ServerThreadMessage::Listening(addr) => {
                        self.is_listening = true;
                        self.server_status = format!("Listening on {}", addr);
                        println!("Server is now listening on {}", addr);
                    }
                    ServerThreadMessage::NewConnection(stream, peer_addr) => {
                        println!("New incoming connection from: {}", peer_addr);
                        if self.is_connected {
                            println!("Already have an active connection. Closing new incoming connection from {}.", peer_addr);
                            drop(stream); 
                        } else {
                            self.tcp_stream = Some(stream);
                            self.is_connected = true;
                            self.peer_address_input.clear(); 
                            self.connection_status = format!("Connected to by {}", peer_addr);
                        }
                    }
                    ServerThreadMessage::BindError(err_msg) => {
                        self.is_listening = false;
                        self.server_status = format!("Server Bind Error: {}", err_msg);
                        eprintln!("Server Bind Error: {}", err_msg);
                        if let Some(handle) = self.tcp_listener_handle.take() {
                            handle.join().expect("Server listener thread (bind error) failed to join");
                        }
                    }
                    ServerThreadMessage::AcceptError(err_msg) => {
                        self.server_status = format!("Server Accept Error: {}", err_msg);
                        eprintln!("Server Accept Error: {}", err_msg);
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
                    // Let's simulate a check for now.
                    // In a real scenario, this might involve trying to rdev::listen
                    // in a non-blocking way or using another method.
                    // For now, we'll assume we need to guide the user if it's the first run or unknown.
                    ui.label("Checking macOS Accessibility permissions...");
                    // Placeholder: In a real app, you'd call a function here that tries
                    // to use rdev or a macOS API to determine status.
                    // For this example, let's just pretend it becomes false if not yet set.
                    // self.macos_accessibility_granted = Some(check_macos_accessibility());
                    // We'll manually set it for demonstration purposes in a real scenario.
                    // For now, let's just show the message.
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
            ui.heading("Input Event Listener Control");

            if self.rdev_listener_handle.is_none() { // If listener is not running
                if ui.button("Start Input Listener").clicked() {
                    let (gui_tx, gui_rx) = channel::<RdevThreadMessage>();
                    self.rdev_gui_rx = Some(gui_rx);
                    
                    let thread_gui_tx_for_spawn = gui_tx.clone();
                    let use_grab = is_running_on_wayland(); // Retain Wayland detection from HEAD

                    if use_grab { // Wayland-specific status message from HEAD
                        self.input_listener_status = "Starting listener (Wayland mode using grab)...".to_string();
                        println!("Attempting to start rdev listener in Wayland (grab) mode.");
                    } else { // X11 status message from HEAD
                        self.input_listener_status = "Starting listener (X11 mode using listen)...".to_string();
                        println!("Attempting to start rdev listener in X11 (listen) mode.");
                    }
                    self.last_event_summary = "Waiting for events...".to_string(); // Common

                    let handle = thread::spawn(move || {
                        let event_tx = thread_gui_tx_for_spawn.clone(); 
                        let status_tx = thread_gui_tx_for_spawn;
                        
                        // Get display size - this logic is from HEAD and seems beneficial
                        let (screen_width, screen_height) = match rdev::display_size() {
                            Ok((w, h)) => (w, h),
                            Err(e) => {
                                let err_msg = format!("Failed to get display size: {:?}. Edge detection will be unreliable.", e);
                                eprintln!("{}", err_msg);
                                status_tx.send(RdevThreadMessage::Error(err_msg)).unwrap_or_default();
                                return; 
                            }
                        };
                        // Send Started message with actual dimensions - from HEAD
                        status_tx.send(RdevThreadMessage::Started { width: screen_width as u32, height: screen_height as u32 }).unwrap_or_default();

                        if use_grab { // Wayland path from HEAD
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
                        } else { // X11 path from HEAD (which is similar to origin/master's listen but better structured)
                            println!("rdev listener thread: Using LISTEN (X11 mode).");
                            if let Err(error) = rdev::listen(move |event| { // rdev::listen is from HEAD
                                if event_tx.send(RdevThreadMessage::Event(event)).is_err() {
                                    println!("rdev (listen): GUI channel closed, can't send event. Listener continues.");
                                }
                            }) {
                                let error_msg = format!("X11 Listener (listen) error: {:?}", error); // Error formatting from HEAD
                                eprintln!("rdev listen error: {}", error_msg);
                                status_tx.send(RdevThreadMessage::Error(error_msg)).unwrap_or_default();
                        } else {
                                println!("rdev listen finished without an explicit error (unexpected).");
                            }
                        }
                    });
                    self.rdev_listener_handle = Some(handle);
                }
            } else { // If listener IS running
                if ui.button("Stop Input Listener").clicked() {
                    println!("Stop Input Listener clicked.");
                    self.input_listener_status = "Stop request sent (input listener).".to_string();
                    if let Some(_handle) = self.rdev_listener_handle.take() {
                        println!("Input Listener thread handle taken.");
                    }
                    self.rdev_thread_tx = None; 
                    self.input_listener_status = "Input Listener abandoned by GUI.".to_string();
                }
            }

            ui.separator();
            ui.label(format!("Input Listener Status: {}", self.input_listener_status));
            ui.label(format!("Last Event: {}", self.last_event_summary));

            ui.separator();
            ui.heading("Mouse & Screen Info");
            ui.label(format!("Screen Dimensions: {}x{}", self.screen_width, self.screen_height));
            ui.label(format!("Mouse Position: ({:.0}, {:.0})", self.current_mouse_x, self.current_mouse_y));
            ui.label(format!("Edge Status: {}", self.edge_status));

            // --- Networking UI Placeholder ---
            ui.separator();
            ui.heading("Networking");

            ui.label("Listen for Incoming Connections:");
            ui.horizontal(|ui| {
                if !self.is_listening && self.tcp_listener_handle.is_none() {
                    if ui.button("Start Listening").clicked() {
                        if self.is_connected { 
                            self.server_status = "Cannot start listening while already connected/connecting.".to_string();
                        } else {
                            self.server_status = format!("Attempting to listen on 0.0.0.0:{}...", DEFAULT_PORT);
                            let (server_tx_channel, server_rx_channel) = channel::<ServerThreadMessage>();
                            self.server_gui_rx = Some(server_rx_channel);
                            
                            let listen_addr = format!("0.0.0.0:{}", DEFAULT_PORT);

                            let handle = thread::spawn(move || {
                                println!("Server listener thread: Attempting to bind to {}", listen_addr);
                                match TcpListener::bind(&listen_addr) {
                                    Ok(listener) => {
                                        if let Ok(local_addr) = listener.local_addr() {
                                            server_tx_channel.send(ServerThreadMessage::Listening(local_addr)).unwrap_or_else(|e| eprintln!("GUI channel closed, can't send Listening status: {}",e));
                                        } else {
                                            server_tx_channel.send(ServerThreadMessage::BindError("Failed to get local address after bind".to_string())).unwrap_or_else(|e| eprintln!("GUI channel closed, can't send BindError: {}",e));
                                            return;
                                        }

                                        println!("Server listener thread: Bound successfully, now accepting connections on {}.", listen_addr);
                                        // Loop to accept connections. This loop will block until a connection is made or an error occurs.
                                        for stream_result in listener.incoming() { 
                                            match stream_result {
                                                Ok(stream) => {
                                                    if let Ok(peer_addr) = stream.peer_addr() {
                                                        println!("Server listener thread: Accepted new connection from {}", peer_addr);
                                                        if server_tx_channel.send(ServerThreadMessage::NewConnection(stream, peer_addr)).is_err() {
                                                            println!("Server listener thread: GUI channel closed, cannot send new connection. Dropping connection & stopping listener thread.");
                                                            break; // Stop accepting if GUI is gone
                                                        }
                                                        // For a simple KVM, we typically want one connection at a time.
                                                        // After successfully sending one connection to the GUI, we can stop this listener thread.
                                                        // The GUI will then manage this one connection.
                                                        // If the GUI wants to listen again, it can press "Start Listening" again (after disconnecting).
                                                        println!("Server listener thread: Sent NewConnection to GUI. Stopping listener thread.");
                                                        break; 
                                                    } else {
                                                        eprintln!("Server listener thread: Accepted connection but failed to get peer address. Dropping.");
                                                        drop(stream); // Close it if peer_addr fails
                                                    }
                                                }
                                                Err(e) => {
                                                    eprintln!("Server listener thread: Error accepting connection: {}", e);
                                                    server_tx_channel.send(ServerThreadMessage::AcceptError(e.to_string())).unwrap_or_else(|e_send| eprintln!("GUI channel closed, can't send AcceptError: {}", e_send));
                                                    break; // Stop on accept error
                                                }
                                            }
                                        }
                                        println!("Server listener thread: Exited accept loop.");
                                    }
                                    Err(e) => {
                                        eprintln!("Server listener thread: Failed to bind to {}: {}", listen_addr, e);
                                        server_tx_channel.send(ServerThreadMessage::BindError(e.to_string())).unwrap_or_else(|e_send| eprintln!("GUI channel closed, can't send BindError: {}", e_send));
                                    }
                                }
                            });
                            self.tcp_listener_handle = Some(handle);
                        }
                    }
                } else if self.is_listening { // is_listening is true OR tcp_listener_handle is Some
                    if ui.button("Stop Listening").clicked() {
                        println!("Stop Listening clicked.");
                        // To properly stop the listener thread that's blocking on `listener.incoming()` or `listener.accept()`,
                        // the ideal way is to close the TcpListener from another thread. 
                        // However, we don't store the TcpListener in MyApp directly. The thread owns it.
                        // A more advanced approach would involve sending a signal to the listener thread to close its listener and exit.
                        // For now, taking the handle is a basic step. The OS will clean up the listening socket when the app exits
                        // or if the thread panics/exits. If the listener thread is stuck in accept(), simply taking the handle
                        // won't stop it immediately. It might error out if the app closes its side of the channel.

                        if let Some(handle) = self.tcp_listener_handle.take() {
                            println!("Server listener thread handle taken. Actual stop depends on thread behavior.");
                            // Note: Joining here would block the GUI if the listener thread doesn't exit quickly.
                            // handle.join().expect("Listener thread panic"); 
                        }
                        self.is_listening = false; // Assume we are stopping
                        self.server_status = "Listener stop requested.".to_string();
                        // We might need to also close self.tcp_stream if it was established via this listener.
                        // For now, the main Disconnect button handles self.tcp_stream.
                    }
                }
                ui.label(format!("Server Status: {}", self.server_status));
            });
            
            ui.label("My Connection Details (for others to connect to me):");
            ui.horizontal(|ui| {
                ui.label(format!("Listening on Port (if active): {}", if self.is_listening { DEFAULT_PORT.to_string() } else { "N/A".to_string() } ));
                ui.label(format!("Local IP Address: {}", self.my_local_ip));
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
                            let mut addr_to_connect = peer_addr_clone.clone(); 
                            if !addr_to_connect.contains(':') {
                                println!("Port not specified for {}, appending default port {}.", addr_to_connect, DEFAULT_PORT);
                                addr_to_connect = format!("{}:{}", addr_to_connect, DEFAULT_PORT);
                            }
 
                            println!("Connection thread: Attempting to connect to {}", addr_to_connect);
                            match TcpStream::connect(&addr_to_connect) {
                                Ok(stream) => {
                                    println!("Connection thread: Successfully connected to {}", addr_to_connect);
                                    conn_tx.send(ConnectionThreadMessage::Connected(stream)).unwrap_or_else(|e| eprintln!("GUI channel closed, can't send Connected: {}",e));
                                }
                                Err(e) => {
                                    eprintln!("Connection thread: Failed to connect to {}: {}", addr_to_connect, e);
                                    conn_tx.send(ConnectionThreadMessage::ConnectionFailed(e.to_string())).unwrap_or_else(|e| eprintln!("GUI channel closed, can't send ConnectionFailed: {}",e));
                                }
                            }
                        });
                        self.connection_thread_handle = Some(handle);
                    } else {
                        self.connection_status = "Error: Peer address is empty.".to_string();
                    }
                }
            } else { 
                if ui.button("Disconnect").clicked() {
                    if let Some(stream) = self.tcp_stream.take() {
                        println!("Disconnecting from peer...");
                        if let Err(e) = stream.shutdown(Shutdown::Both) {
                            eprintln!("Error shutting down TCP stream: {}", e);
                        }
                    }
                    self.is_connected = false;
                    self.connection_status = "Disconnected".to_string();
                    if let Some(handle) = self.connection_thread_handle.take() {
                        handle.join().expect("Connection thread (during disconnect) failed to join");
                    }
                    // If we disconnect, we should probably allow listening again if it was stopped due to a connection.
                    // For now, the user has to manually restart listening.
                }
            }
            ui.label(format!("Connection Status: {}", self.connection_status));
            // --- End Networking UI Placeholder ---

        });

        // Request a repaint if the listener might be active, to process messages
        if self.rdev_listener_handle.is_some() || self.connection_thread_handle.is_some() || self.tcp_listener_handle.is_some() {
            ctx.request_repaint();
        }
    }
}

const DEFAULT_PORT: u16 = 7878;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([600.0, 900.0]), // Increased height
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
//     }
//     // return true; if it didn't error in a way that indicates denial
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
