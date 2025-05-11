// Module for macOS specific utility functions 

use eframe::egui;

pub fn check_and_display_macos_accessibility(
    macos_accessibility_granted: &mut Option<bool>,
    ui: &mut egui::Ui,
) {
    if cfg!(target_os = "macos") {
        if macos_accessibility_granted.is_none() {
            // In a real scenario, this might involve trying to rdev::listen
            // in a non-blocking way or using another method.
            ui.label("Checking macOS Accessibility permissions...");
            // Placeholder for actual check. For now, we rely on user interaction via buttons below
            // or an initial manual setting in MyApp::default() if we had a real check.
        }

        match macos_accessibility_granted {
            Some(true) => {
                ui.colored_label(egui::Color32::GREEN, "macOS Accessibility permissions appear to be granted.");
            }
            Some(false) => {
                ui.colored_label(egui::Color32::RED, "macOS Accessibility permissions are NOT granted.");
                ui.label("To use this application fully, please grant Accessibility access.");
                ui.label("Go to System Settings > Privacy & Security > Accessibility.");
                ui.label("Click the '+' button, find this application (or your terminal if running via 'cargo run'), and enable it.");
                if ui.button("Open Accessibility Settings").clicked() {
                    let _ = std::process::Command::new("open")
                        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                        .status();
                }
            }
            None => {
                ui.label("macOS Accessibility permission status is unknown.");
                ui.label("Please ensure permissions are granted as per instructions above.");
                if ui.button("Re-check Accessibility (Simulated - Set to Denied for Demo)").clicked() {
                    *macos_accessibility_granted = Some(false); // Simulate finding it's denied
                }
                if ui.button("Simulate Permission Granted").clicked() {
                    *macos_accessibility_granted = Some(true);
                }
            }
        }
    } else {
        ui.label("Not on macOS, no specific accessibility permission check needed here.");
    }
} 