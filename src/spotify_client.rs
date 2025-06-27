use anyhow::{anyhow, Context, Result};
use rspotify::{
    model::{CurrentlyPlayingContext, PlayableItem, TrackId},
    prelude::*,
    scopes, AuthCodeSpotify, Config, Credentials, OAuth,
};
use tracing::{info, warn};
use url::Url;

use crate::config::AppConfig;

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub id: Option<String>,
    pub name: String,
    pub artist: String,
}

pub struct SpotifyManager {
    client: AuthCodeSpotify,
    current_track: Option<TrackInfo>,
}

impl SpotifyManager {
    pub async fn new(config: &AppConfig) -> Result<Self> {
        let creds = Credentials::new(&config.spotify.client_id, &config.spotify.client_secret);
        
        let oauth = OAuth {
            redirect_uri: config.spotify.redirect_uri.clone(),
            scopes: scopes!(
                "user-read-currently-playing",
                "user-read-playback-state",
                "user-library-modify",
                "user-library-read"
            ),
            ..Default::default()
        };
        
        let cache_dir = dirs::cache_dir()
            .context("Failed to get cache directory")?
            .join("spotify-quick-actions");
        
        // Create cache directory if it doesn't exist
        std::fs::create_dir_all(&cache_dir)
            .context("Failed to create cache directory")?;
        
        let spotify_config = Config {
            token_cached: true,
            cache_path: cache_dir.join("token.json"),
            ..Default::default()
        };
        
        let mut client = AuthCodeSpotify::with_config(creds, oauth, spotify_config);
        
        // Try to load cached token first
        if let Err(_) = client.read_token_cache(false).await {
            info!("No cached token found, starting OAuth flow");
            Self::authenticate(&mut client).await?;
        } else {
            info!("Loaded cached token");
            
            // Test the token
            if let Err(_) = client.current_user().await {
                warn!("Cached token is invalid, re-authenticating");
                Self::authenticate(&mut client).await?;
            }
        }
        
        info!("Successfully authenticated with Spotify");
        
        Ok(Self {
            client,
            current_track: None,
        })
    }
    
    async fn authenticate(client: &mut AuthCodeSpotify) -> Result<()> {
        let url = client.get_authorize_url(false)?;
        
        info!("Opening browser for Spotify authentication...");
        println!("\n🔐 Spotify Authentication Required");
        println!("1. Your browser will open to Spotify's login page");
        println!("2. Log in and authorize the application");
        println!("3. After authorization, you'll be redirected to a page that won't load");
        println!("4. Copy the ENTIRE URL from your browser's address bar");
        println!("5. Paste it here when prompted\n");
        
        webbrowser::open(&url).context("Failed to open browser")?;
        
        println!("Please paste the redirect URL here (after authorizing in browser):");
        
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let redirect_url = input.trim();
        
        // Parse authorization code from the URL
        let parsed_url = Url::parse(redirect_url)
            .context("Invalid URL. Please make sure you copied the complete URL from your browser.")?;
        
        let mut code = None;
        for (key, value) in parsed_url.query_pairs() {
            if key == "code" {
                code = Some(value.into_owned());
                break;
            }
        }
        
        if let Some(code) = code {
            client.request_token(&code).await?;
            client.write_token_cache().await?;
            info!("Authentication successful and token cached");
            println!("✅ Authentication successful! The application will now start.\n");
            Ok(())
        } else {
            Err(anyhow!("No authorization code found in URL. Please make sure you copied the complete redirect URL."))
        }
    }
    
    pub async fn get_current_track(&mut self) -> Result<TrackInfo> {
        let currently_playing = self
            .client
            .current_playing(None, None::<Vec<_>>)
            .await
            .context("Failed to get currently playing track")?;
        
        match currently_playing {
            Some(CurrentlyPlayingContext {
                item: Some(PlayableItem::Track(track)),
                ..
            }) => {
                let track_info = TrackInfo {
                    id: track.id.as_ref().map(|id| {
                        let id_str = id.to_string();
                        info!("Raw track ID from Spotify: {}", id_str);
                        id_str
                    }),
                    name: track.name.clone(),
                    artist: track.artists.first()
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| "Unknown Artist".to_string()),
                };
                
                self.current_track = Some(track_info.clone());
                Ok(track_info)
            }
            _ => Err(anyhow!("No track currently playing"))
        }
    }
    
    pub async fn like_current_track(&mut self) -> Result<TrackInfo> {
        let track_info = self.get_current_track().await?;
        
        if let Some(track_id_str) = &track_info.id {
            // Parse track ID - try different formats
            let track_id = if track_id_str.starts_with("spotify:track:") {
                // Use from_uri for full Spotify URI
                TrackId::from_uri(track_id_str)?
            } else if track_id_str.len() == 22 {
                // Raw Spotify ID (22 characters) - use from_id
                TrackId::from_id(track_id_str)?
            } else {
                // Try as-is, fall back to just the last part if it's a URL
                match TrackId::from_id(track_id_str) {
                    Ok(id) => id,
                    Err(_) => {
                        // Extract ID from URL format
                        let clean_id = track_id_str
                            .split('/')
                            .last()
                            .unwrap_or(track_id_str)
                            .split('?')
                            .next()
                            .unwrap_or(track_id_str);
                        TrackId::from_id(clean_id)?
                    }
                }
            };
            
            self.client
                .current_user_saved_tracks_add([track_id])
                .await
                .context("Failed to like track")?;
            
            Ok(track_info)
        } else {
            Err(anyhow!("Current track has no ID"))
        }
    }
    
    pub async fn save_current_track(&mut self) -> Result<TrackInfo> {
        // For Spotify, "saving" and "liking" are the same action
        // Both add the track to the user's "Liked Songs" library
        self.like_current_track().await
    }
    
}