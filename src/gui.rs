// Module for UI drawing logic 

use eframe::egui;
use crate::event_handling::{RdevThreadMessage, spawn_rdev_listener_thread, is_running_on_wayland};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread::JoinHandle;
use crate::network_client::{ConnectionThreadMessage, spawn_connect_thread};
use crate::network_server::{ServerThreadMessage, spawn_listener_thread};
use crate::constants::DEFAULT_PORT;
use std::net::{TcpStream, Shutdown};

pub fn draw_input_listener_panel(
    ui: &mut egui::Ui,
    rdev_listener_handle: &mut Option<JoinHandle<()>>,
    input_listener_status: &mut String,
    last_event_summary: &mut String,
    rdev_gui_rx: &mut Option<Receiver<RdevThreadMessage>>,
    rdev_thread_tx: &mut Option<Sender<RdevThreadMessage>> // For stopping/signaling the thread if needed
) {
    ui.heading("Input Event Listener Control");

    if rdev_listener_handle.is_none() { // If listener is not running
        if ui.button("Start Input Listener").clicked() {
            let (gui_tx_channel, gui_rx_channel) = channel::<RdevThreadMessage>();
            *rdev_gui_rx = Some(gui_rx_channel);
            
            let use_grab = is_running_on_wayland();
            if use_grab {
                *input_listener_status = "Starting listener (Wayland mode using grab)...".to_string();
                println!("Attempting to start rdev listener in Wayland (grab) mode.");
            } else {
                *input_listener_status = "Starting listener (X11 mode using listen)...".to_string();
                println!("Attempting to start rdev listener in X11 (listen) mode.");
            }
            *last_event_summary = "Waiting for events...".to_string();

            let handle = spawn_rdev_listener_thread(gui_tx_channel.clone());
            *rdev_listener_handle = Some(handle);
            // rdev_thread_tx is not typically set here, but on demand if we need to send a message to it.
            // *rdev_thread_tx = Some(gui_tx_channel); // This might be needed if we want to send stop signals
        }
    } else { // If listener IS running
        if ui.button("Stop Input Listener").clicked() {
            println!("Stop Input Listener clicked.");
            *input_listener_status = "Stop request sent (input listener).".to_string();
            if let Some(_handle) = rdev_listener_handle.take() {
                println!("Input Listener thread handle taken.");
                // Here, if we had a way to signal the thread to stop, we'd use rdev_thread_tx.send(...)
                // For now, rdev::grab and rdev::listen don't have a built-in non-blocking stop mechanism
                // that's easily triggered from here other than dropping the handle or process exit.
            }
            *rdev_thread_tx = None; // Clear any sender we might have had.
            *input_listener_status = "Input Listener abandoned by GUI.".to_string();
        }
    }

    ui.separator();
    ui.label(format!("Input Listener Status: {}", input_listener_status));
    ui.label(format!("Last Event: {}", last_event_summary));
}

pub fn draw_mouse_screen_info_panel(
    ui: &mut egui::Ui,
    screen_width: u32,
    screen_height: u32,
    current_mouse_x: f64,
    current_mouse_y: f64,
    edge_status: &str // Using &str as it's likely from a String but only needs to be read
) {
    ui.heading("Mouse & Screen Info");
    ui.label(format!("Screen Dimensions (from rdev): {}x{}", screen_width, screen_height));
    ui.label(format!("Current Mouse Position: ({:.0}, {:.0})", current_mouse_x, current_mouse_y));
    ui.label(format!("Edge Status: {}", edge_status));
}

pub fn draw_networking_panel(
    ui: &mut egui::Ui,
    // Listener state
    is_listening: &mut bool,
    tcp_listener_handle: &mut Option<JoinHandle<()>>,
    server_status: &mut String,
    server_gui_rx: &mut Option<Receiver<ServerThreadMessage>>,
    // Connection state
    is_connected: &mut bool,
    peer_address_input: &mut String,
    connection_thread_handle: &mut Option<JoinHandle<()>>,
    connection_status: &mut String,
    connection_gui_rx: &mut Option<Receiver<ConnectionThreadMessage>>,
    tcp_stream: &mut Option<TcpStream>,
    // Display info
    my_local_ip: &str,
) {
    ui.heading("Networking");

    // --- Listen for Incoming Connections ---
    ui.label("Listen for Incoming Connections:");
    ui.horizontal(|ui| {
        if !*is_listening && tcp_listener_handle.is_none() {
            if ui.button("Start Listening").clicked() {
                if *is_connected { 
                    *server_status = "Cannot start listening while already connected/connecting.".to_string();
                } else {
                    *server_status = format!("Attempting to listen on 0.0.0.0:{}...", DEFAULT_PORT);
                    let (server_tx_c, server_rx_c) = channel::<ServerThreadMessage>();
                    *server_gui_rx = Some(server_rx_c);
                    
                    let handle = spawn_listener_thread(server_tx_c);
                    *tcp_listener_handle = Some(handle);
                }
            }
        } else if *is_listening { // Simplified condition, is_listening should be the primary source of truth
            if ui.button("Stop Listening").clicked() {
                println!("Stop Listening clicked.");
                if let Some(_handle) = tcp_listener_handle.take() {
                    println!("Server listener thread handle taken. Actual stop depends on thread behavior.");
                }
                *is_listening = false; 
                *server_status = "Listener stop requested.".to_string();
            }
        }
        ui.label(format!("Server Status: {}", server_status));
    });
    
    // --- My Connection Details ---
    ui.label("My Connection Details (for others to connect to me):");
    ui.horizontal(|ui| {
        ui.label(format!("Listening on Port (if active): {}", if *is_listening { DEFAULT_PORT.to_string() } else { "N/A".to_string() } ));
        ui.label(format!("Local IP Address: {}", my_local_ip));
    });

    ui.separator();

    // --- Connect to Peer ---
    ui.label("Connect to Peer:");
    ui.horizontal(|ui| {
        ui.label("Peer Address/ID (e.g., 127.0.0.1:7878):");
        ui.add_enabled(!*is_connected && connection_thread_handle.is_none(), 
            egui::TextEdit::singleline(peer_address_input)
        );
    });

    if connection_thread_handle.is_some() {
        ui.label("Processing connection attempt...");
    } else if !*is_connected {
        if ui.button("Connect").clicked() {
            if !peer_address_input.is_empty() {
                *connection_status = format!("Attempting to connect to {}...", peer_address_input);
                let (conn_tx_c, conn_rx_c) = channel::<ConnectionThreadMessage>();
                *connection_gui_rx = Some(conn_rx_c);
                
                let handle = spawn_connect_thread(peer_address_input.clone(), conn_tx_c);
                *connection_thread_handle = Some(handle);
            } else {
                *connection_status = "Error: Peer address is empty.".to_string();
            }
        }
    } else { // is_connected is true
        if ui.button("Disconnect").clicked() {
            if let Some(stream_ref) = tcp_stream.take() { // take to get ownership for shutdown
                println!("Disconnecting from peer...");
                if let Err(e) = stream_ref.shutdown(Shutdown::Both) {
                    eprintln!("Error shutting down TCP stream: {}", e);
                }
                // tcp_stream is now None
            }
            *is_connected = false;
            *connection_status = "Disconnected".to_string();
            if let Some(handle) = connection_thread_handle.take() {
                // This handle is for the thread that *initiated* the connection.
                // It should have exited after sending Connected or ConnectionFailed.
                // Joining here is usually quick.
                handle.join().expect("Connection thread (during disconnect) failed to join");
            }
        }
    }
    ui.label(format!("Connection Status: {}", connection_status));
}

pub fn draw_header_panel(ui: &mut egui::Ui, app_value: &mut i32) {
    ui.heading("Rust KVM Experiment");
    ui.horizontal(|ui| {
        ui.label("Placeholder value:");
        ui.add(egui::DragValue::new(app_value));
    });
} 