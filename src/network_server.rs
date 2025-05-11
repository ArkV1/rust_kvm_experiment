// Module for incoming TCP listener logic 

use std::net::{TcpListener, TcpStream, SocketAddr};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};

#[derive(Debug)]
pub enum ServerThreadMessage { // Made public
    Listening(SocketAddr),
    NewConnection(TcpStream, SocketAddr), // Carries the new stream and the peer's address
    BindError(String),
    AcceptError(String),
    // We might add ListenerStopped if we make stop more graceful
}

pub fn spawn_listener_thread(
    server_tx_channel: Sender<ServerThreadMessage>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let listen_addr = format!("0.0.0.0:{}", crate::constants::DEFAULT_PORT);
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
    })
} 