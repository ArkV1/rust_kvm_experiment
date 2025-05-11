// Module for outgoing TCP connection logic 

use std::net::TcpStream;
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};

#[derive(Debug)]
pub enum ConnectionThreadMessage { // Made public
    Connected(TcpStream),
    ConnectionFailed(String),
    Disconnected, // This might be signalled by the read loop later, or if connect itself can tell.
}

pub fn spawn_connect_thread(
    peer_address_input: String,
    conn_tx: Sender<ConnectionThreadMessage>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut addr_to_connect = peer_address_input.clone();
        if !addr_to_connect.contains(':') {
            println!("Port not specified for {}, appending default port {}.", addr_to_connect, crate::constants::DEFAULT_PORT);
            addr_to_connect = format!("{}:{}", addr_to_connect, crate::constants::DEFAULT_PORT);
        }

        println!("Connection thread: Attempting to connect to {}", addr_to_connect);
        match TcpStream::connect(&addr_to_connect) {
            Ok(stream) => {
                println!("Connection thread: Successfully connected to {}", addr_to_connect);
                // The stream needs to be set up for non-blocking if we want to send Disconnected from here
                // For now, Connected is the primary success message from this thread.
                // If the connection drops later, it's usually detected by a read/write error in the main app.
                if let Err(e) = conn_tx.send(ConnectionThreadMessage::Connected(stream)) {
                    eprintln!("GUI channel closed for connect thread, can't send Connected: {}",e);
                }
            }
            Err(e) => {
                eprintln!("Connection thread: Failed to connect to {}: {}", addr_to_connect, e);
                if let Err(e_send) = conn_tx.send(ConnectionThreadMessage::ConnectionFailed(e.to_string())) {
                     eprintln!("GUI channel closed for connect thread, can't send ConnectionFailed: {}",e_send);
                }
            }
        }
    })
} 