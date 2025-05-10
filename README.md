# Rust KVM Experiment

This project is an experiment to create a software KVM (Keyboard, Video, Mouse) switch in Rust, allowing the use of a single keyboard and mouse across multiple computers. The goal is to achieve a seamless experience, similar to an extended desktop, with control switching based on mouse movement to screen edges.

A key design principle is a peer-to-peer architecture where any connected machine can potentially share its input devices, and local physical input on any machine always takes priority, allowing a user to instantly reclaim control of their own computer.

## Current Status

*   Basic cross-platform input event listening is functional using the `rdev` crate.
*   A simple GUI has been implemented using `eframe` (`egui`).
*   The GUI includes placeholder UI for macOS Accessibility permission guidance.
*   The `rdev` event listener is not yet integrated into the GUI's lifecycle (i.e., not started/stopped by GUI actions).

## Development Setup

1.  **Install Rust:** Follow the official instructions at [rust-lang.org](https://www.rust-lang.org/tools/install).
2.  **Clone the repository (if applicable).**
3.  **Dependencies:** The project uses `rdev` for input event handling and `eframe` for the GUI. These will be downloaded automatically by Cargo.

## Running the Project

1.  Navigate to the project directory: `cd rust_kvm_experiment`
2.  Run the application: `cargo run`

### macOS Specific Setup: Accessibility Permissions

For the application to capture global keyboard and mouse events on macOS during development (when using `cargo run`), you **must** grant Accessibility access to the terminal application or IDE you are using to execute `cargo run`.

1.  Go to **System Settings > Privacy & Security > Accessibility**.
2.  Click the **+** button.
3.  Add your terminal application (e.g., `Terminal.app`, `iTerm.app`, `Visual Studio Code.app`, `Cursor.app`, etc.) to the list.
4.  Ensure the toggle next to it is **enabled**.
5.  **Important:** You may need to **quit and restart** your terminal/IDE for the new permissions to take effect.

When the application is eventually bundled into a `.app` package, that `.app` itself will be what users add to this list.

## Next Steps / Roadmap

*   **Integrate `rdev` listener into GUI:**
    *   Start/stop the listener from the GUI.
    *   Run the listener in a separate thread to avoid freezing the GUI.
    *   Communicate events or status from the listener thread to the GUI thread (e.g., using channels).
*   **Implement macOS Accessibility Check:** Programmatically check (if feasible and reliable) if permissions are granted and update the GUI accordingly, rather than relying on simulation buttons.
*   **Screen Edge Detection:**
    *   Use `rdev::display_size()` to get screen dimensions.
    *   Process `MouseMove` events to determine when the cursor reaches a screen edge.
*   **Input Simulation:**
    *   Implement sending input events using `rdev::simulate()` on a target machine.
*   **Networking & Discovery:**
    *   **Peer-to-Peer Layer:** Design and implement a robust peer-to-peer networking layer (e.g., using TCP or UDP with a library like `tokio`).
    *   **Unique Client IDs:** Assign a unique ID to each application instance.
    *   **Direct Connections:** Support direct connection to peers on the same LAN via IP address or resolvable hostname/domain name.
    *   **ID-Based Discovery (Optional Server):** Design and implement an optional central discovery server.
        *   Clients register their Unique ID with the server.
        *   Clients can query the server to find other clients by ID, facilitating connections across different networks.
    *   **LAN Discovery (e.g., mDNS/Bonjour):** Explore LAN-based discovery as an alternative or complement to the ID server for easy local connections.
    *   **Secure Communication:** Implement encryption for network traffic.
    *   **Data Serialization:** Define a clear format for serializing/deserializing event data and control messages for network transmission.
*   **Core KVM Logic (Peer-to-Peer with Local Priority):**
    *   Manage connections between peers.
    *   Handle transfer of input control based on screen edge transitions.
    *   Implement the "local physical input has priority" rule on each machine.
*   **Configuration & Virtual Display:**
    *   **Virtual Display Space:** Allow users to graphically arrange discovered/connected clients in a virtual multi-monitor layout within the GUI. This map is crucial for screen edge switching.
    *   **Save/Load Layouts:** Persist and load user-defined virtual display configurations.
    *   **Network Settings:** Configure discovery server details, preferred connection methods, etc.
*   **Cross-Platform Testing & Refinements:** Thoroughly test and refine on Windows and Linux.
*   **Application Bundling:**
    *   Create distributable application bundles for macOS (`.app`), Windows (`.exe` installer), and Linux (e.g., AppImage, deb, rpm).
*   **Error Handling & Logging:** Robust error handling and useful logging.
*   **Clipboard Sharing (Optional Enhancement).**
*   **Drag and Drop (Optional Enhancement).**

---

This README provides a starting point. We can expand it as the project evolves. 