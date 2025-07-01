use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, io::{self, Write}, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub spotify: SpotifyConfig,
    pub hotkeys: HotkeyConfig,
    pub notifications: NotificationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub like_track: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub enabled: bool,
    pub timeout_ms: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            spotify: SpotifyConfig {
                client_id: "YOUR_SPOTIFY_CLIENT_ID".to_string(),
                client_secret: "YOUR_SPOTIFY_CLIENT_SECRET".to_string(),
                redirect_uri: "https://example.com/callback".to_string(),
            },
            hotkeys: HotkeyConfig {
                like_track: "Ctrl+Alt+L".to_string(),
            },
            notifications: NotificationConfig {
                enabled: true,
                timeout_ms: 3000,
            },
        }
    }
}

impl AppConfig {
    pub fn load_or_create() -> Result<Self> {
        let config_path = Self::config_file_path()?;
        
        if config_path.exists() {
            let config_str = fs::read_to_string(&config_path)
                .context("Failed to read config file")?;
            
            let config: Self = toml::from_str(&config_str)
                .context("Failed to parse config file")?;
            
            // Validate Spotify credentials
            if config.spotify.client_id == "YOUR_SPOTIFY_CLIENT_ID" {
                eprintln!("⚠️  Please update your Spotify credentials in: {}", config_path.display());
                eprintln!("   1. Go to https://developer.spotify.com/dashboard");
                eprintln!("   2. Create a new app");
                eprintln!("   3. Set redirect URI to: https://example.com/callback");
                eprintln!("   4. Copy Client ID and Client Secret to the config file");
                std::process::exit(1);
            }
            
            Ok(config)
        } else {
            eprintln!("📝 No config file found. Let's set up your Spotify credentials.");
            eprintln!("");
            eprintln!("   Setup instructions:");
            eprintln!("   1. Go to https://developer.spotify.com/dashboard");
            eprintln!("   2. Create a new app");
            eprintln!("   3. Set redirect URI to: https://example.com/callback");
            eprintln!("   4. Copy Client ID and Client Secret below");
            eprintln!("");
            
            let client_id = Self::prompt_for_input("Enter your Spotify Client ID: ")?;
            let client_secret = Self::prompt_for_input("Enter your Spotify Client Secret: ")?;
            
            let config = Self {
                spotify: SpotifyConfig {
                    client_id,
                    client_secret,
                    redirect_uri: "https://example.com/callback".to_string(),
                },
                hotkeys: HotkeyConfig {
                    like_track: "Ctrl+Alt+L".to_string(),
                },
                notifications: NotificationConfig {
                    enabled: true,
                    timeout_ms: 3000,
                },
            };
            
            config.save()?;
            eprintln!("✅ Config file created at: {}", config_path.display());
            eprintln!("🚀 Starting application...");
            eprintln!("");
            
            Ok(config)
        }
    }
    
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_file_path()?;
        
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .context("Failed to create config directory")?;
        }
        
        let config_str = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        
        fs::write(&config_path, config_str)
            .context("Failed to write config file")?;
        
        Ok(())
    }
    
    fn config_file_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .context("Failed to get config directory")?;
        
        Ok(config_dir.join("spotify-quick-actions").join("config.toml"))
    }
    
    fn prompt_for_input(prompt: &str) -> Result<String> {
        print!("{}", prompt);
        io::stdout().flush().context("Failed to flush stdout")?;
        
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("Failed to read input")?;
        
        let trimmed = input.trim().to_string();
        if trimmed.is_empty() {
            anyhow::bail!("Input cannot be empty");
        }
        
        Ok(trimmed)
    }
}