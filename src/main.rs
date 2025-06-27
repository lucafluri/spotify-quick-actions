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

use config::AppConfig;
use spotify_client::SpotifyManager;

#[derive(Debug, Clone)]
pub enum AppMessage {
    LikeCurrentTrack,
    SaveCurrentTrack,
    ShowCurrentTrack,
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
    let save_hotkey = HotKey::new(
        Some(Modifiers::CONTROL | Modifiers::ALT),
        Code::KeyS,
    );
    
    hotkey_manager
        .register(like_hotkey)
        .context("Failed to register like hotkey (Ctrl+Alt+L)")?;
    hotkey_manager
        .register(save_hotkey)
        .context("Failed to register save hotkey (Ctrl+Alt+S)")?;
    
    info!("Registered global hotkeys: Ctrl+Alt+L (like), Ctrl+Alt+S (save)");
    
    // Create system tray
    let tray_menu = Menu::new();
    
    let current_track_item = MenuItem::new("No track playing", false, None);
    let save_item = MenuItem::new("💾 Save Current Track", true, None);
    let separator = PredefinedMenuItem::separator();
    let quit_item = MenuItem::new("Quit", true, None);
    
    // Capture menu item IDs before moving into tray
    let save_item_id = save_item.id();
    let quit_item_id = quit_item.id();
    
    tray_menu.append_items(&[
        &current_track_item,
        &separator,
        &save_item,
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
                        info!("Now playing: {} - {}", current_track.name, current_track.artist);
                    }
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
                .body(&format!("{} - {}", track_info.name, track_info.artist))
                .timeout(3000)
                .show();
            info!("Liked track: {} - {}", track_info.name, track_info.artist);
        }
        Err(e) => {
            error!("Failed to like track: {}", e);
        }
    }
}

async fn handle_save_track(spotify_manager: Arc<Mutex<SpotifyManager>>) {
    let mut manager = spotify_manager.lock().await;
    
    match manager.save_current_track().await {
        Ok(track_info) => {
            let _ = Notification::new()
                .summary("💾 Saved!")
                .body(&format!("{} - {}", track_info.name, track_info.artist))
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

fn create_tray_icon() -> tray_icon::Icon {
    // Create a simple 16x16 green circle icon
    let size = 16;
    let mut rgba = Vec::with_capacity(size * size * 4);
    
    for _ in 0..(size * size) {
        rgba.extend_from_slice(&[0u8, 255u8, 0u8, 255u8]); // Green RGBA
    }
    
    tray_icon::Icon::from_rgba(rgba, 16, 16).expect("Failed to create icon")
}