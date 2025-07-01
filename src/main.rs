use anyhow::{Context, Result};
use global_hotkey::{hotkey::{Code, HotKey, Modifiers}, GlobalHotKeyEvent, GlobalHotKeyManager};
use notify_rust::Notification;
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, Mutex};
use tray_icon::{
    menu::{Menu, MenuItem, PredefinedMenuItem, MenuEvent},
    TrayIconBuilder, TrayIconEvent,
};
use tracing::{error, info, warn};
use winit::event_loop::EventLoop;

mod config;
mod spotify_client;

#[cfg(windows)]
mod autostart;

use config::AppConfig;
use spotify_client::SpotifyManager;

#[derive(Debug, Clone)]
pub enum AppMessage {
    LikeCurrentTrack,
    UnlikeCurrentTrack,
    SaveCurrentTrack,
    ShowCurrentTrack,
    ToggleAutostart,
    UpdateTrayWithTrack(String), // Track info for tray display
    UpdateAutostartStatus(String), // Update autostart menu item text
    UpdateTrayMenu, // Rebuild entire menu with current state
    Quit,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    info!("Starting Spotify Quick Actions");

    // Load or create config
    let config = AppConfig::load_or_create().context("Failed to load configuration")?;
    
    // Create event loop for system tray (must be on main thread)
    let event_loop = EventLoop::new().context("Failed to create event loop")?;
    
    // Create message channel
    let (tx, mut rx) = mpsc::unbounded_channel::<AppMessage>();
    
    // Initialize Spotify client
    let spotify_manager = Arc::new(Mutex::new(
        SpotifyManager::new(&config).await.context("Failed to initialize Spotify client")?
    ));
    
    // Setup global hotkeys
    let hotkey_manager = GlobalHotKeyManager::new().context("Failed to create hotkey manager")?;
    
    // Register hotkeys
    let like_hotkey = HotKey::new(
        Some(Modifiers::CONTROL | Modifiers::ALT),
        Code::KeyL,
    );
    let unlike_hotkey = HotKey::new(
        Some(Modifiers::CONTROL | Modifiers::ALT),
        Code::KeyU,
    );
    let save_hotkey = HotKey::new(
        Some(Modifiers::CONTROL | Modifiers::ALT),
        Code::KeyS,
    );
    
    hotkey_manager
        .register(like_hotkey)
        .context("Failed to register like hotkey (Ctrl+Alt+L)")?;
    hotkey_manager
        .register(unlike_hotkey)
        .context("Failed to register unlike hotkey (Ctrl+Alt+U)")?;
    hotkey_manager
        .register(save_hotkey)
        .context("Failed to register save hotkey (Ctrl+Alt+S)")?;
    
    info!("Registered global hotkeys: Ctrl+Alt+L (like), Ctrl+Alt+U (unlike), Ctrl+Alt+S (save)");
    
    // Create system tray
    let tray_menu = Menu::new();
    
    let current_track_item = MenuItem::new("No track playing", false, None);
    let save_item = MenuItem::new("💾 Save Current Track", true, None);
    let unlike_item = MenuItem::new("💔 Remove Current Track", true, None);
    let separator = PredefinedMenuItem::separator();
    
    // Create autostart item with current status
    #[cfg(windows)]
    let autostart_text = autostart::get_autostart_status_text();
    #[cfg(not(windows))]
    let autostart_text = "❌ Autostart: Not supported".to_string();
    let autostart_item = MenuItem::new(&autostart_text, true, None);
    
    let quit_item = MenuItem::new("Quit", true, None);
    
    // Capture menu item references for dynamic updates
    let current_track_item_ref = Arc::new(current_track_item.clone());
    let autostart_item_ref = Arc::new(autostart_item.clone());
    let _current_track_item_id = current_track_item.id();
    let save_item_id = save_item.id();
    let unlike_item_id = unlike_item.id();
    let autostart_item_id = autostart_item.id();
    let quit_item_id = quit_item.id();
    
    tray_menu.append_items(&[
        &current_track_item,
        &separator,
        &save_item,
        &unlike_item,
        &separator,
        &autostart_item,
        &separator,
        &quit_item,
    ])?;
    
    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Spotify Quick Actions")
        .with_icon(create_tray_icon())
        .build()
        .context("Failed to create system tray icon")?;
    
    // Clone sender for hotkey thread
    let hotkey_tx = tx.clone();
    
    // Spawn hotkey listener thread
    thread::spawn(move || {
        let global_hotkey_channel = GlobalHotKeyEvent::receiver();
        let mut last_like_time = Instant::now() - Duration::from_secs(10); // Initialize to allow first trigger
        let mut last_unlike_time = Instant::now() - Duration::from_secs(10);
        let mut last_save_time = Instant::now() - Duration::from_secs(10);
        let debounce_duration = Duration::from_millis(500); // 500ms debounce
        
        loop {
            if let Ok(event) = global_hotkey_channel.recv() {
                let now = Instant::now();
                
                if event.id == like_hotkey.id() {
                    if now.duration_since(last_like_time) >= debounce_duration {
                        last_like_time = now;
                        let _ = hotkey_tx.send(AppMessage::LikeCurrentTrack);
                    }
                } else if event.id == unlike_hotkey.id() {
                    if now.duration_since(last_unlike_time) >= debounce_duration {
                        last_unlike_time = now;
                        let _ = hotkey_tx.send(AppMessage::UnlikeCurrentTrack);
                    }
                } else if event.id == save_hotkey.id() {
                    if now.duration_since(last_save_time) >= debounce_duration {
                        last_save_time = now;
                        let _ = hotkey_tx.send(AppMessage::SaveCurrentTrack);
                    }
                }
            }
        }
    });
    
    // Clone references for the async task
    let spotify_manager_clone = Arc::clone(&spotify_manager);
    let spotify_tx = tx.clone();
    
    // Spawn Spotify management task
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        let mut last_track_id: Option<String> = None;
        
        loop {
            interval.tick().await;
            
            let mut manager = spotify_manager_clone.lock().await;
            
            // Update current track info
            if let Ok(current_track) = manager.get_current_track().await {
                if let Some(track_id) = &current_track.id {
                    if Some(track_id.clone()) != last_track_id {
                        last_track_id = Some(track_id.clone());
                        let track_display = format!("🎵 {} - {}", current_track.name, current_track.artist);
                        info!("Now playing: {}", track_display);
                        
                        // Send message to update tray menu item
                        let _ = spotify_tx.send(AppMessage::UpdateTrayWithTrack(track_display));
                    }
                }
            } else {
                // No track playing, reset if we had one before
                if last_track_id.is_some() {
                    last_track_id = None;
                    let _ = spotify_tx.send(AppMessage::UpdateTrayWithTrack("No track playing".to_string()));
                }
            }
        }
    });
    
    // Handle tray events and messages
    let tray_tx = tx.clone();
    
    event_loop.run(move |_event, elwt| {
        // Handle tray icon events
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Click { .. } => {
                    let _ = tray_tx.send(AppMessage::ShowCurrentTrack);
                }
                _ => {}
            }
        }
        
        // Handle menu events separately
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == save_item_id {
                let _ = tray_tx.send(AppMessage::SaveCurrentTrack);
            } else if event.id == unlike_item_id {
                let _ = tray_tx.send(AppMessage::UnlikeCurrentTrack);
            } else if event.id == autostart_item_id {
                let _ = tray_tx.send(AppMessage::ToggleAutostart);
            } else if event.id == quit_item_id {
                let _ = tray_tx.send(AppMessage::Quit);
            }
        }
        
        // Handle application messages
        if let Ok(msg) = rx.try_recv() {
            let spotify_manager = Arc::clone(&spotify_manager);
            
            match msg {
                AppMessage::LikeCurrentTrack => {
                    tokio::spawn(async move {
                        handle_like_track(spotify_manager).await;
                    });
                }
                AppMessage::UnlikeCurrentTrack => {
                    tokio::spawn(async move {
                        handle_unlike_track(spotify_manager).await;
                    });
                }
                AppMessage::SaveCurrentTrack => {
                    tokio::spawn(async move {
                        handle_save_track(spotify_manager).await;
                    });
                }
                AppMessage::ShowCurrentTrack => {
                    tokio::spawn(async move {
                        handle_show_current_track(spotify_manager).await;
                    });
                }
                AppMessage::ToggleAutostart => {
                    let tx_clone = tx.clone();
                    tokio::spawn(async move {
                        handle_toggle_autostart(tx_clone).await;
                    });
                }
                AppMessage::UpdateTrayWithTrack(track_info) => {
                    // Update the current track menu item
                    current_track_item_ref.set_text(&track_info);
                }
                AppMessage::UpdateAutostartStatus(status_text) => {
                    // Update the autostart menu item
                    autostart_item_ref.set_text(&status_text);
                }
                AppMessage::UpdateTrayMenu => {
                    // Reserved for future use - complete menu rebuild
                }
                AppMessage::Quit => {
                    info!("Shutting down...");
                    elwt.exit();
                }
            }
        }
    })?;
    
    Ok(())
}

async fn handle_like_track(spotify_manager: Arc<Mutex<SpotifyManager>>) {
    let mut manager = spotify_manager.lock().await;
    
    match manager.like_current_track().await {
        Ok(track_info) => {
            let _ = Notification::new()
                .summary("❤️ Liked!")
                .body(&format!("✅ Verified: {} - {}", track_info.name, track_info.artist))
                .timeout(3000)
                .show();
            info!("Liked track: {} - {}", track_info.name, track_info.artist);
        }
        Err(e) => {
            error!("Failed to like track: {}", e);
            let _ = Notification::new()
                .summary("❌ Failed to like track")
                .body(&e.to_string())
                .timeout(3000)
                .show();
        }
    }
}

async fn handle_unlike_track(spotify_manager: Arc<Mutex<SpotifyManager>>) {
    let mut manager = spotify_manager.lock().await;
    
    match manager.unlike_current_track().await {
        Ok(track_info) => {
            let _ = Notification::new()
                .summary("💔 Removed!")
                .body(&format!("✅ Verified: {} - {}", track_info.name, track_info.artist))
                .timeout(3000)
                .show();
            info!("Unliked track: {} - {}", track_info.name, track_info.artist);
        }
        Err(e) => {
            error!("Failed to unlike track: {}", e);
            let _ = Notification::new()
                .summary("❌ Failed to remove track")
                .body(&e.to_string())
                .timeout(3000)
                .show();
        }
    }
}

async fn handle_save_track(spotify_manager: Arc<Mutex<SpotifyManager>>) {
    let mut manager = spotify_manager.lock().await;
    
    match manager.save_current_track().await {
        Ok(track_info) => {
            let _ = Notification::new()
                .summary("💾 Saved!")
                .body(&format!("✅ Verified: {} - {}", track_info.name, track_info.artist))
                .timeout(3000)
                .show();
            info!("Saved track: {} - {}", track_info.name, track_info.artist);
        }
        Err(e) => {
            error!("Failed to save track: {}", e);
            let _ = Notification::new()
                .summary("❌ Failed to save track")
                .body(&e.to_string())
                .timeout(3000)
                .show();
        }
    }
}

async fn handle_show_current_track(spotify_manager: Arc<Mutex<SpotifyManager>>) {
    let mut manager = spotify_manager.lock().await;
    
    match manager.get_current_track().await {
        Ok(track_info) => {
            info!("Current track: {} - {}", track_info.name, track_info.artist);
        }
        Err(e) => {
            warn!("No track currently playing: {}", e);
        }
    }
}

async fn handle_toggle_autostart(tx: mpsc::UnboundedSender<AppMessage>) {
    #[cfg(windows)]
    {
        match autostart::toggle_autostart() {
            Ok(new_state) => {
                let status = if new_state { "enabled" } else { "disabled" };
                info!("Autostart {}", status);
                
                // Send message to update the menu item text
                let new_text = if new_state {
                    "✅ Autostart: Enabled"
                } else {
                    "⏹️ Autostart: Disabled"
                };
                let _ = tx.send(AppMessage::UpdateAutostartStatus(new_text.to_string()));
                
                let _ = Notification::new()
                    .summary("⚙️ Autostart Settings")
                    .body(&format!("Autostart has been {}", status))
                    .timeout(3000)
                    .show();
            }
            Err(e) => {
                error!("Failed to toggle autostart: {}", e);
                let _ = tx.send(AppMessage::UpdateAutostartStatus("❓ Autostart: Error".to_string()));
                let _ = Notification::new()
                    .summary("❌ Autostart Error")
                    .body(&format!("Failed to change autostart setting: {}", e))
                    .timeout(3000)
                    .show();
            }
        }
    }
    
    #[cfg(not(windows))]
    {
        warn!("Autostart is not supported on this platform");
        let _ = tx.send(AppMessage::UpdateAutostartStatus("❌ Autostart: Not supported".to_string()));
        let _ = Notification::new()
            .summary("❌ Not Supported")
            .body("Autostart is only supported on Windows")
            .timeout(3000)
            .show();
    }
}

fn create_tray_icon() -> tray_icon::Icon {
    // Create a simple 16x16 green circle icon
    let size = 16;
    let mut rgba = Vec::with_capacity(size * size * 4);
    
    for _ in 0..(size * size) {
        rgba.extend_from_slice(&[0u8, 255u8, 0u8, 255u8]); // Green RGBA
    }
    
    tray_icon::Icon::from_rgba(rgba, 16, 16).expect("Failed to create icon")
}