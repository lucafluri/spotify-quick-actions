use anyhow::{anyhow, Context, Result};
use rspotify::{
    model::{CurrentlyPlayingContext, PlayableItem, TrackId},
    prelude::*,
    scopes, AuthCodeSpotify, Config, Credentials, OAuth,
};
use std::{fs, path::PathBuf, time::Duration};
use tokio::time::sleep;
use tracing::{info, warn, error};
use url::Url;

use crate::config::AppConfig;

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub id: Option<String>,
    pub name: String,
    pub artist: String,
    pub uri: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub success: bool,
    pub track_info: TrackInfo,
    pub verified_after_ms: u64,
    pub attempts: u32,
}

pub struct SpotifyManager {
    client: AuthCodeSpotify,
    verification_delay_ms: u64,
    max_verification_attempts: u32,
}

impl SpotifyManager {
    /// Create a new Spotify manager with verification
    pub async fn new(config: &AppConfig) -> Result<Self> {
        Self::with_config(config, 1000, 8).await  // Increased delay and attempts
    }
    
    /// Create a new Spotify manager with forced re-authentication
    pub async fn new_with_fresh_auth(config: &AppConfig) -> Result<Self> {
        // Clear any existing cache first
        if let Ok(cache_path) = Self::get_token_cache_path() {
            let _ = std::fs::remove_file(cache_path);
            info!("🗑️ Cleared existing token cache to force fresh authentication");
        }
        Self::with_config(config, 750, 3).await
    }
    
    /// Create with custom verification settings
    pub async fn with_config(
        config: &AppConfig,
        verification_delay_ms: u64,
        max_verification_attempts: u32
    ) -> Result<Self> {
        let creds = Credentials::new(&config.spotify.client_id, &config.spotify.client_secret);
        
        let oauth = OAuth {
            redirect_uri: config.spotify.redirect_uri.clone(),
            scopes: scopes!(
                "user-read-currently-playing",
                "user-read-playback-state",
                "user-library-modify",
                "user-library-read",
                "user-read-private"
            ),
            ..Default::default()
        };
        
        let cache_path = Self::get_token_cache_path()?;
        
        let config = Config {
            token_cached: true,           // Enable persistent token caching
            token_refreshing: true,       // Enable automatic token refresh
            cache_path,
            ..Default::default()
        };
        
        let mut client = AuthCodeSpotify::with_config(creds, oauth, config);
        
        // Handle authentication with persistent tokens
        Self::ensure_authenticated(&mut client).await?;
        
        Ok(Self {
            client,
            verification_delay_ms,
            max_verification_attempts,
        })
    }
    
    /// Ensure client is authenticated, handling token refresh automatically
    async fn ensure_authenticated(client: &mut AuthCodeSpotify) -> Result<()> {
        // Try to load cached token first
        match client.read_token_cache(false).await {
            Ok(Some(token)) => {
                info!("📁 Loaded cached token");
                
                // Debug: Log token details (without exposing sensitive data)
                info!("🔍 Token debug - access_token length: {}, refresh_token present: {}, expires_at: {:?}", 
                    token.access_token.len(),
                    token.refresh_token.is_some(),
                    token.expires_at);
                
                if let Some(ref refresh_token) = token.refresh_token {
                    info!("🔍 Refresh token length: {}", refresh_token.len());
                } else {
                    warn!("🔍 Refresh token is None");
                }
                
                // Check if we have both access and refresh tokens
                if token.access_token.is_empty() {
                    warn!("❌ Cached token is missing access token, re-authenticating...");
                    Self::authenticate_first_time(client).await?;
                    return Ok(());
                }
                
                if token.refresh_token.is_none() || token.refresh_token.as_ref().unwrap().is_empty() {
                    warn!("❌ Cached token is missing refresh token, re-authenticating...");
                    Self::authenticate_first_time(client).await?;
                    return Ok(());
                }
                
                // CRITICAL FIX: Set the token in the client's internal state
                // The read_token_cache only reads from file but doesn't set it in the client
                *client.get_token().lock().await.unwrap() = Some(token.clone());
                info!("🔧 Token set in client internal state");
                
                // Test the token by making a simple API call
                match client.current_user().await {
                    Ok(user) => {
                        info!("✅ Token is valid for user: {}", 
                            user.display_name.unwrap_or_else(|| "Unknown".to_string()));
                    }
                    Err(_) => {
                        warn!("🔄 Token expired, attempting refresh...");
                        match client.refresh_token().await {
                            Ok(_) => {
                                info!("✅ Token refreshed successfully");
                                client.write_token_cache().await
                                    .context("Failed to save refreshed token")?;
                            }
                            Err(e) => {
                                warn!("❌ Token refresh failed: {}, need to re-authenticate", e);
                                Self::authenticate_first_time(client).await?;
                            }
                        }
                    }
                }
            }
            Ok(None) => {
                info!("🔐 No cached token found, starting initial authentication...");
                Self::authenticate_first_time(client).await?;
            }
            Err(e) => {
                warn!("❌ Failed to read token cache: {}, starting initial authentication...", e);
                Self::authenticate_first_time(client).await?;
            }
        }
        
        Ok(())
    }
    
    /// Handle first-time authentication (only runs once)
    async fn authenticate_first_time(client: &mut AuthCodeSpotify) -> Result<()> {
        // Clear any existing invalid cache by removing cached file
        if let Ok(cache_path) = Self::get_token_cache_path() {
            let _ = std::fs::remove_file(cache_path);
        }
        
        let url = client.get_authorize_url(true)?;  // Use state parameter for security
        
        println!("\n🔐 Spotify Authentication Required (One-time setup)");
        println!("1. Your browser will open to Spotify's login page");
        println!("2. Log in and authorize the application");
        println!("3. You'll be redirected to a page that won't load - that's normal!");
        println!("4. Copy the ENTIRE URL from your browser's address bar");
        println!("5. Paste it here when prompted\n");
        
        // Open browser automatically
        if let Err(e) = webbrowser::open(&url) {
            warn!("Failed to open browser automatically: {}", e);
            println!("Please manually open this URL: {}", url);
        }
        
        // Get redirect URL from user
        println!("Paste the redirect URL here:");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let redirect_url = input.trim();
        
        // Parse authorization code from the URL
        let parsed_url = Url::parse(redirect_url)
            .context("Invalid URL. Please make sure you copied the complete URL from your browser.")?;
        
        let code = parsed_url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.into_owned())
            .ok_or_else(|| anyhow!("No authorization code found in URL. Please make sure you copied the complete redirect URL."))?;
        
        // Exchange authorization code for tokens
        client.request_token(&code).await
            .context("Failed to exchange authorization code for tokens")?;
        
        // Immediately check if we got a refresh token
        match client.get_token().lock().await.unwrap().as_ref() {
            Some(token) => {
                info!("🔍 Token obtained - access_token length: {}, refresh_token present: {}", 
                    token.access_token.len(),
                    token.refresh_token.is_some());
                
                if token.refresh_token.is_none() {
                    return Err(anyhow!("Authorization completed but no refresh token was provided by Spotify. This may be due to an app configuration issue."));
                }
                
                if let Some(ref refresh_token) = token.refresh_token {
                    if refresh_token.is_empty() {
                        return Err(anyhow!("Authorization completed but refresh token is empty."));
                    }
                    info!("✅ Valid refresh token obtained (length: {})", refresh_token.len());
                }
            }
            None => {
                return Err(anyhow!("Authorization completed but no token was set in client"));
            }
        }
        
        // Save tokens to cache for future use
        client.write_token_cache().await
            .context("Failed to save tokens to cache")?;
        
        // Verify the token was saved correctly by reading it back
        match client.read_token_cache(false).await {
            Ok(Some(token)) => {
                let has_access = !token.access_token.is_empty();
                let has_refresh = token.refresh_token.is_some() && !token.refresh_token.as_ref().unwrap().is_empty();
                info!("Token verification: access_token={}, refresh_token={}", has_access, has_refresh);
                
                if !has_access || !has_refresh {
                    warn!("⚠️ Saved token is incomplete - this may cause re-authentication on next run");
                }
            }
            Ok(None) => {
                warn!("⚠️ No token found after saving - this may cause re-authentication on next run");
            }
            Err(e) => {
                warn!("⚠️ Failed to verify saved token: {} - this may cause re-authentication on next run", e);
            }
        }
        
        println!("✅ Authentication successful! Token cached for future use.");
        println!("🎉 You'll never need to authenticate again (unless you revoke access)!\n");
        
        Ok(())
    }
    
    /// Get the path for token cache
    fn get_token_cache_path() -> Result<PathBuf> {
        let cache_dir = dirs::cache_dir()
            .context("Failed to get system cache directory")?;
        
        let app_cache_dir = cache_dir.join("spotify-quick-actions");
        fs::create_dir_all(&app_cache_dir)
            .context("Failed to create application cache directory")?;
        
        Ok(app_cache_dir.join("spotify_token.json"))
    }
    
    /// Ensure token is valid before making API calls
    async fn ensure_token_valid(&mut self) -> Result<()> {
        // The rspotify library with token_refreshing: true should handle this automatically,
        // but we can add an extra check if needed
        match self.client.current_user().await {
            Ok(_) => Ok(()),
            Err(_) => {
                warn!("🔄 Token validation failed, attempting refresh...");
                self.client.refresh_token().await
                    .context("Failed to refresh token")?;
                self.client.write_token_cache().await
                    .context("Failed to save refreshed token")?;
                info!("✅ Token refreshed successfully");
                Ok(())
            }
        }
    }
    
    /// Get current playing track
    pub async fn get_current_track(&mut self) -> Result<TrackInfo> {
        self.ensure_token_valid().await?;
        
        let currently_playing = self.client
            .current_playing(None, None::<Vec<_>>)
            .await
            .context("Failed to get currently playing track")?;
        
        match currently_playing {
            Some(CurrentlyPlayingContext {
                item: Some(PlayableItem::Track(track)),
                ..
            }) => {
                let track_info = TrackInfo {
                    id: track.id.as_ref().map(|id| id.to_string()),
                    name: track.name.clone(),
                    artist: track.artists.first()
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| "Unknown Artist".to_string()),
                    uri: track.id.as_ref().map(|id| format!("spotify:track:{}", id.id())),
                };
                
                info!("Current track: {} - {}", track_info.name, track_info.artist);
                Ok(track_info)
            }
            _ => Err(anyhow!("No track currently playing"))
        }
    }
    
    /// Like current track with verification
    pub async fn like_current_track(&mut self) -> Result<TrackInfo> {
        self.ensure_token_valid().await?;
        
        let track_info = self.get_current_track().await?;
        
        if let Some(track_id_str) = &track_info.id {
            let track_id = self.parse_track_id(track_id_str)?;
            
            info!("🎯 Attempting to LIKE track: {} - {} (ID: {})", track_info.name, track_info.artist, track_id.id());
            
            // Attempt to like the track
            self.client
                .current_user_saved_tracks_add([track_id.clone()])
                .await
                .context("Failed to add track to saved tracks")?;
            
            info!("📡 LIKE API call completed, starting verification...");
            
            // Verify the operation with retries
            let verification_result = self.verify_track_liked(&track_id, &track_info).await?;
            
            if verification_result.success {
                info!("✅ Successfully liked and verified: {} - {} (verified in {}ms after {} attempts)", 
                    track_info.name, track_info.artist, 
                    verification_result.verified_after_ms,
                    verification_result.attempts);
                Ok(track_info)
            } else {
                error!("❌ Failed to verify track was liked: {} - {}", track_info.name, track_info.artist);
                Err(anyhow!("Track like operation failed verification - the track may not have been saved to your library"))
            }
        } else {
            Err(anyhow!("Current track has no ID"))
        }
    }
    
    /// Save current track (alias for like_current_track for compatibility)
    pub async fn save_current_track(&mut self) -> Result<TrackInfo> {
        self.like_current_track().await
    }
    
    /// Unlike current track with verification
    pub async fn unlike_current_track(&mut self) -> Result<TrackInfo> {
        self.ensure_token_valid().await?;
        
        let track_info = self.get_current_track().await?;
        
        if let Some(track_id_str) = &track_info.id {
            let track_id = self.parse_track_id(track_id_str)?;
            
            info!("🎯 Attempting to UNLIKE track: {} - {} (ID: {})", track_info.name, track_info.artist, track_id.id());
            
            // Attempt to unlike the track
            self.client
                .current_user_saved_tracks_delete([track_id.clone()])
                .await
                .context("Failed to remove track from saved tracks")?;
            
            info!("📡 UNLIKE API call completed, starting verification...");
            
            // Verify the operation with retries
            let verification_result = self.verify_track_unliked(&track_id, &track_info).await?;
            
            if verification_result.success {
                info!("✅ Successfully unliked and verified: {} - {} (verified in {}ms after {} attempts)", 
                    track_info.name, track_info.artist,
                    verification_result.verified_after_ms,
                    verification_result.attempts);
                Ok(track_info)
            } else {
                error!("❌ Failed to verify track was unliked: {} - {}", track_info.name, track_info.artist);
                Err(anyhow!("Track unlike operation failed verification - the track may still be in your library"))
            }
        } else {
            Err(anyhow!("Current track has no ID"))
        }
    }
    
    /// Check if a track is currently liked
    pub async fn is_track_liked(&mut self, track_id: &TrackId<'_>) -> Result<bool> {
        self.ensure_token_valid().await?;
        
        info!("🔍 Checking if track is liked: {}", track_id.id());
        
        let is_saved = self.client
            .current_user_saved_tracks_contains([track_id.clone()])
            .await
            .context("Failed to check if track is saved")?;
        
        let result = is_saved.first() == Some(&true);
        info!("🔍 Track liked status: {} = {}", track_id.id(), result);
        
        Ok(result)
    }
    
    /// Verify that a like operation succeeded with enhanced retry logic
    async fn verify_track_liked(&mut self, track_id: &TrackId<'_>, track_info: &TrackInfo) -> Result<VerificationResult> {
        let start_time = std::time::Instant::now();
        info!("🔍 Starting verification for LIKE operation: {} - {}", track_info.name, track_info.artist);
        
        // Check current state before starting verification
        match self.is_track_liked(track_id).await {
            Ok(true) => {
                info!("✅ Track was already liked before verification attempts");
                return Ok(VerificationResult {
                    success: true,
                    track_info: track_info.clone(),
                    verified_after_ms: start_time.elapsed().as_millis() as u64,
                    attempts: 0,
                });
            }
            Ok(false) => {
                info!("⏳ Track not yet liked, starting verification attempts...");
            }
            Err(e) => {
                warn!("⚠️ Initial verification check failed: {}", e);
            }
        }
        
        for attempt in 1..=self.max_verification_attempts {
            // Progressive delay: start with base delay, increase each attempt
            let delay = self.verification_delay_ms + (attempt - 1) as u64 * 500;
            info!("⏳ Verification attempt {}/{} - waiting {}ms...", attempt, self.max_verification_attempts, delay);
            sleep(Duration::from_millis(delay)).await;
            
            match self.is_track_liked(track_id).await {
                Ok(true) => {
                    let elapsed_ms = start_time.elapsed().as_millis() as u64;
                    info!("✅ LIKE verified successfully after {}ms and {} attempts", elapsed_ms, attempt);
                    return Ok(VerificationResult {
                        success: true,
                        track_info: track_info.clone(),
                        verified_after_ms: elapsed_ms,
                        attempts: attempt,
                    });
                }
                Ok(false) => {
                    warn!("❌ Attempt {}/{}: Track still not liked, retrying...", attempt, self.max_verification_attempts);
                    
                    // If we're on the last few attempts, try re-liking the track
                    if attempt >= self.max_verification_attempts - 2 {
                        warn!("🔄 Re-attempting like operation on attempt {}", attempt);
                        if let Err(e) = self.client.current_user_saved_tracks_add([track_id.clone()]).await {
                            warn!("⚠️ Re-like attempt failed: {}", e);
                        } else {
                            info!("🔄 Re-like operation completed");
                        }
                    }
                }
                Err(e) => {
                    warn!("⚠️ Attempt {}/{}: Verification API call failed: {}", attempt, self.max_verification_attempts, e);
                }
            }
        }
        
        let elapsed_ms = start_time.elapsed().as_millis() as u64;
        error!("❌ LIKE verification failed after {} attempts and {}ms", self.max_verification_attempts, elapsed_ms);
        Ok(VerificationResult {
            success: false,
            track_info: track_info.clone(),
            verified_after_ms: elapsed_ms,
            attempts: self.max_verification_attempts,
        })
    }
    
    /// Verify that an unlike operation succeeded with enhanced retry logic
    async fn verify_track_unliked(&mut self, track_id: &TrackId<'_>, track_info: &TrackInfo) -> Result<VerificationResult> {
        let start_time = std::time::Instant::now();
        info!("🔍 Starting verification for UNLIKE operation: {} - {}", track_info.name, track_info.artist);
        
        // Check current state before starting verification
        match self.is_track_liked(track_id).await {
            Ok(false) => {
                info!("✅ Track was already unliked before verification attempts");
                return Ok(VerificationResult {
                    success: true,
                    track_info: track_info.clone(),
                    verified_after_ms: start_time.elapsed().as_millis() as u64,
                    attempts: 0,
                });
            }
            Ok(true) => {
                info!("⏳ Track still liked, starting verification attempts...");
            }
            Err(e) => {
                warn!("⚠️ Initial verification check failed: {}", e);
            }
        }
        
        for attempt in 1..=self.max_verification_attempts {
            // Progressive delay: start with base delay, increase each attempt
            let delay = self.verification_delay_ms + (attempt - 1) as u64 * 500;
            info!("⏳ Verification attempt {}/{} - waiting {}ms...", attempt, self.max_verification_attempts, delay);
            sleep(Duration::from_millis(delay)).await;
            
            match self.is_track_liked(track_id).await {
                Ok(false) => {
                    let elapsed_ms = start_time.elapsed().as_millis() as u64;
                    info!("✅ UNLIKE verified successfully after {}ms and {} attempts", elapsed_ms, attempt);
                    return Ok(VerificationResult {
                        success: true,
                        track_info: track_info.clone(),
                        verified_after_ms: elapsed_ms,
                        attempts: attempt,
                    });
                }
                Ok(true) => {
                    warn!("❌ Attempt {}/{}: Track still liked, retrying...", attempt, self.max_verification_attempts);
                    
                    // If we're on the last few attempts, try re-unliking the track
                    if attempt >= self.max_verification_attempts - 2 {
                        warn!("🔄 Re-attempting unlike operation on attempt {}", attempt);
                        if let Err(e) = self.client.current_user_saved_tracks_delete([track_id.clone()]).await {
                            warn!("⚠️ Re-unlike attempt failed: {}", e);
                        } else {
                            info!("🔄 Re-unlike operation completed");
                        }
                    }
                }
                Err(e) => {
                    warn!("⚠️ Attempt {}/{}: Verification API call failed: {}", attempt, self.max_verification_attempts, e);
                }
            }
        }
        
        let elapsed_ms = start_time.elapsed().as_millis() as u64;
        error!("❌ UNLIKE verification failed after {} attempts and {}ms", self.max_verification_attempts, elapsed_ms);
        Ok(VerificationResult {
            success: false,
            track_info: track_info.clone(),
            verified_after_ms: elapsed_ms,
            attempts: self.max_verification_attempts,
        })
    }
    
    /// Parse various track ID formats
    fn parse_track_id<'a>(&self, track_id_str: &'a str) -> Result<TrackId<'a>> {
        // Handle different track ID formats
        if track_id_str.starts_with("spotify:track:") {
            TrackId::from_uri(track_id_str)
                .context("Failed to parse Spotify track URI")
        } else if track_id_str.len() == 22 && track_id_str.chars().all(|c| c.is_alphanumeric()) {
            TrackId::from_id(track_id_str)
                .context("Failed to parse raw Spotify track ID")
        } else {
            // Try to extract ID from URL or other formats
            let clean_id = track_id_str
                .split('/')
                .last()
                .unwrap_or(track_id_str)
                .split('?')
                .next()
                .unwrap_or(track_id_str);
            
            TrackId::from_id(clean_id)
                .context("Failed to parse track ID from URL format")
        }
    }
    
    /// Get current user info (useful for testing authentication)
    pub async fn get_current_user(&mut self) -> Result<rspotify::model::PrivateUser> {
        self.ensure_token_valid().await?;
        Ok(self.client.current_user().await?)
    }
    
    /// Force a token refresh (useful for testing)
    pub async fn refresh_token(&mut self) -> Result<()> {
        self.client.refresh_token().await?;
        self.client.write_token_cache().await?;
        info!("✅ Token manually refreshed");
        Ok(())
    }
    
    /// Clear the token cache and force re-authentication on next use
    pub fn clear_token_cache() -> Result<()> {
        let cache_path = Self::get_token_cache_path()?;
        if cache_path.exists() {
            std::fs::remove_file(&cache_path)
                .context("Failed to remove token cache file")?;
            info!("🗑️ Token cache cleared at: {}", cache_path.display());
        } else {
            info!("ℹ️ No token cache file found to clear");
        }
        Ok(())
    }
    
    /// Check the current token cache status
    pub async fn check_token_cache_status() -> Result<()> {
        let cache_path = Self::get_token_cache_path()?;
        
        if !cache_path.exists() {
            println!("❌ No token cache file found at: {}", cache_path.display());
            return Ok(());
        }
        
        // Try to read and parse the cache file
        match std::fs::read_to_string(&cache_path) {
            Ok(content) => {
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(json) => {
                        println!("✅ Token cache file found at: {}", cache_path.display());
                        println!("📊 Cache contents:");
                        println!("  - access_token: {}", 
                            json.get("access_token").and_then(|v| v.as_str()).map(|s| if s.is_empty() { "empty" } else { "present" }).unwrap_or("missing"));
                        println!("  - refresh_token: {}", 
                            json.get("refresh_token").and_then(|v| v.as_str()).map(|s| if s.is_empty() { "empty" } else { "present" }).unwrap_or("missing"));
                        println!("  - expires_at: {}", 
                            json.get("expires_at").and_then(|v| v.as_str()).unwrap_or("missing"));
                    }
                    Err(e) => {
                        println!("❌ Token cache file is corrupted: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("❌ Failed to read token cache file: {}", e);
            }
        }
        
        Ok(())
    }
}