# 🎵 Spotify Quick Actions

A blazingly fast Rust application that runs in the background and allows you to instantly like/unlike your current Spotify track with global hotkeys and system tray integration.

## ✨ Features

- **🚀 Ultra-fast performance**: Sub-100ms response times
- **⌨️ Global hotkeys**: 
  - `Ctrl+Alt+L` - Like current track
  - `Ctrl+Alt+U` - Unlike current track
- **🖱️ System tray integration**: Right-click menu with all actions
- **🔄 Real-time track display**: Shows currently playing song in tray menu
- **🚀 Windows autostart**: Toggle autostart on/off from tray menu
- **🔔 Toast notifications**: Instant feedback with verification
- **🛡️ Secure authentication**: OAuth2 flow with persistent token caching
- **📱 Background service**: Runs silently with minimal resource usage (<10MB RAM)
- **✅ Operation verification**: Ensures like/unlike operations actually succeed

## 🚀 Quick Start

### Prerequisites

1. **Rust** (latest stable version)
2. **Spotify Premium account** (required for Web API access)
3. **Windows** (currently Windows-only due to system tray dependencies)

### Installation

1. **Clone the repository**:
   ```bash
   git clone https://github.com/yourusername/spotify-quick-actions.git
   cd spotify-quick-actions
   ```

2. **Build the application**:
   ```bash
   cargo build --release
   ```

3. **The executable will be available at**:
   ```
   target/release/spotify-quick-actions.exe
   ```

### Spotify App Setup

Before using the application, you need to create a Spotify app:

1. **Go to [Spotify Developer Dashboard](https://developer.spotify.com/dashboard)**
2. **Click "Create app"**
3. **Fill in the details**:
   - App name: `Spotify Quick Actions` (or any name you prefer)
   - App description: `Personal hotkey app for liking tracks`
   - Website: `http://localhost` (can be anything)
   - Redirect URI: `http://localhost:8888/callback`
   - API/SDKs: Check `Web API`
4. **Save the app**
5. **Copy your `Client ID` and `Client Secret`**

### Configuration

1. **Run the application once** to generate the default config:
   ```bash
   ./target/release/spotify-quick-actions.exe
   ```

2. **Edit the configuration file** at:
   ```
   %APPDATA%\spotify-quick-actions\config.toml
   ```

3. **Update with your Spotify app credentials**:
   ```toml
   [spotify]
   client_id = "your_spotify_client_id"
   client_secret = "your_spotify_client_secret"  
   redirect_uri = "http://localhost:8888/callback"

   [hotkeys]
   like_track = "Ctrl+Alt+L"

   [notifications]
   enabled = true
   timeout_ms = 3000
   ```

### First Run & Authentication

1. **Start the application**:
   ```bash
   ./target/release/spotify-quick-actions.exe
   ```

2. **Authentication flow** (first time only):
   - Your browser will open to Spotify's login page
   - Log in and authorize the application
   - Copy the entire redirect URL from your browser
   - Paste it into the terminal when prompted
   - Authentication tokens are cached for future use

3. **The app is now running** in your system tray!

## 🎯 Usage

### Global Hotkeys

- **`Ctrl+Alt+L`**: Like/save the currently playing track
- **`Ctrl+Alt+U`**: Unlike/remove the currently playing track

### System Tray Menu

Right-click the tray icon to access:
- **Current track display**: Shows what's currently playing
- **💾 Save Current Track**: Like the current track
- **💔 Remove Current Track**: Unlike the current track
- **✅/⏹️ Autostart**: Toggle Windows startup behavior
- **ℹ️ Hotkeys & Info**: Show hotkey reference
- **Quit**: Exit the application

### Notifications

When you like/unlike a track, you'll see notifications like:
- ✅ **"❤️ Liked! ✅ Verified: Song - Artist"**
- ✅ **"💔 Removed! ✅ Verified: Song - Artist"**
- ❌ **Error messages** if operations fail

## 🔧 Advanced Usage

### Autostart Configuration

- Click the autostart menu item to toggle Windows startup
- When enabled, the app starts automatically with Windows
- Status is shown in the tray menu: "✅ Autostart: Enabled"

### Verification System

The app uses a robust verification system:
- **8 retry attempts** with progressive delays
- **Automatic re-operation** if verification fails
- **Only reports success** when actually verified
- **Detailed logging** for troubleshooting

### Token Management

- Tokens are automatically cached in `%APPDATA%\spotify-quick-actions\`
- Automatic token refresh when expired
- No need to re-authenticate unless you revoke access

## 🛠️ Building from Source

### Dependencies

```toml
[dependencies]
tokio = { version = "1.0", features = ["full"] }
rspotify = { version = "0.13", features = ["client-reqwest"] }
reqwest = { version = "0.11", features = ["json"] }
anyhow = "1.0"
tracing = "0.1"
notify-rust = "4.10"
global-hotkey = "0.5"
tray-icon = "0.14"
winit = "0.29"
dirs = "5.0"
webbrowser = "0.8"
url = "2.5"
```

### Build Commands

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run with logging
RUST_LOG=info cargo run

# Install as system binary
cargo install --path .
```

## 📁 File Locations

- **Config**: `%APPDATA%\spotify-quick-actions\config.toml`
- **Token cache**: `%APPDATA%\spotify-quick-actions\spotify_token.json`
- **Logs**: Console output (use `RUST_LOG=info` for detailed logs)

## 🐛 Troubleshooting

### Authentication Issues

1. **"No cached token found"**: Normal on first run
2. **"Token refresh failed"**: Delete token cache and re-authenticate
3. **"Failed to parse redirect URL"**: Ensure you copy the complete URL

### Hotkey Issues

1. **Hotkeys not working**: Check if another app is using the same combination
2. **Permission errors**: Run as administrator if needed
3. **No response**: Check if Spotify is running and playing music

### Spotify API Issues

1. **"No track currently playing"**: Start playing music in Spotify
2. **"Failed to add track"**: Ensure you have Spotify Premium
3. **"Verification failed"**: Check internet connection and Spotify app status

### Debug Mode

Run with detailed logging:
```bash
RUST_LOG=debug ./target/release/spotify-quick-actions.exe
```

## 🔒 Security & Privacy

- **Local storage only**: All data stays on your machine
- **Secure OAuth2**: Industry-standard authentication
- **Minimal permissions**: Only accesses necessary Spotify data
- **No telemetry**: No data is sent to third parties
- **Open source**: Code is fully auditable

## 📄 License

MIT License - see [LICENSE](LICENSE) file for details.

---

**Made with ❤️ and ⚡ Rust**