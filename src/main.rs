mod event_handling;
mod network_client;
mod network_server;
mod gui;
mod macos_utils;
mod constants;

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

use crate::event_handling::{NetworkKvmMessage, SerializableRdevEvent, SerializableRdevEventType, RdevThreadMessage, is_running_on_wayland, spawn_rdev_listener_thread};
use crate::network_client::{ConnectionThreadMessage, spawn_connect_thread};
use crate::network_server::{ServerThreadMessage, spawn_listener_thread};
use crate::constants::DEFAULT_PORT; // Assuming DEFAULT_PORT is used from constants
use crate::macos_utils::check_and_display_macos_accessibility; // Added
use crate::gui::{draw_header_panel, draw_input_listener_panel, draw_mouse_screen_info_panel, draw_networking_panel}; // Added draw_header_panel

// --- Network Message Definitions ---
// pub enum NetworkKvmMessage { ... }
// pub struct SerializableRdevEvent { ... }
// pub enum SerializableRdevEventType { ... }
// impl From<RdevEvent> for SerializableRdevEvent { ... }
// --- End Network Message Definitions ---

// Define a message type to send from the rdev thread to the GUI thread
// enum RdevThreadMessage { ... }

// Helper function to check for Wayland environment
// fn is_running_on_wayland() -> bool { ... }

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
                                // Reverted to direct comparison with screen_width/height and a small buffer
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
            draw_header_panel(ui, &mut self.value);
            ui.separator();

            check_and_display_macos_accessibility(&mut self.macos_accessibility_granted, ui);
            ui.separator();

            draw_input_listener_panel(
                ui,
                &mut self.rdev_listener_handle,
                &mut self.input_listener_status,
                &mut self.last_event_summary,
                &mut self.rdev_gui_rx,
                &mut self.rdev_thread_tx
            );
            ui.separator();

            draw_mouse_screen_info_panel(
                ui,
                self.screen_width,
                self.screen_height,
                self.current_mouse_x,
                self.current_mouse_y,
                &self.edge_status
            );
            ui.separator();

            draw_networking_panel(
                ui,
                &mut self.is_listening,
                &mut self.tcp_listener_handle,
                &mut self.server_status,
                &mut self.server_gui_rx,
                &mut self.is_connected,
                &mut self.peer_address_input,
                &mut self.connection_thread_handle,
                &mut self.connection_status,
                &mut self.connection_gui_rx,
                &mut self.tcp_stream,
                &self.my_local_ip,
            );
        });

        // Request a repaint if the listener might be active, to process messages
        if self.rdev_listener_handle.is_some() || self.connection_thread_handle.is_some() || self.tcp_listener_handle.is_some() {
            ctx.request_repaint();
        }
    }
}

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
