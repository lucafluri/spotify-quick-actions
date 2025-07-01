use anyhow::{Context, Result};
use std::env;
use tracing::{info, warn};

#[cfg(windows)]
use windows::Win32::System::Registry::{
    RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, RegQueryValueExW, RegCloseKey,
    HKEY_CURRENT_USER, REG_SZ, KEY_ALL_ACCESS, KEY_READ, HKEY, REG_OPEN_CREATE_OPTIONS,
};
#[cfg(windows)]
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
#[cfg(windows)]
use windows::core::HSTRING;

const REGISTRY_KEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run";
const APP_NAME: &str = "SpotifyQuickActions";

/// Check if autostart is currently enabled
pub fn is_autostart_enabled() -> Result<bool> {
    #[cfg(windows)]
    {
        unsafe {
            let key_name = HSTRING::from(REGISTRY_KEY);
            let value_name = HSTRING::from(APP_NAME);
            
            let mut hkey = HKEY::default();
            let result = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &key_name,
                0,
                None,
                REG_OPEN_CREATE_OPTIONS(0),
                KEY_READ,
                None,
                &mut hkey,
                None,
            );
            
            if result.is_err() {
                return Ok(false);
            }
            
            let mut buffer_size = 0u32;
            let query_result = RegQueryValueExW(
                hkey,
                &value_name,
                None,
                None,
                None,
                Some(&mut buffer_size),
            );
            
            let _ = RegCloseKey(hkey);
            
            Ok(query_result.is_ok() && buffer_size > 0)
        }
    }
    
    #[cfg(not(windows))]
    {
        // For non-Windows platforms, autostart is not supported
        Ok(false)
    }
}

/// Enable autostart by adding registry entry
pub fn enable_autostart() -> Result<()> {
    #[cfg(windows)]
    {
        let exe_path = env::current_exe()
            .context("Failed to get current executable path")?;
        
        let exe_path_str = exe_path.to_string_lossy();
        info!("Enabling autostart for: {}", exe_path_str);
        
        unsafe {
            let key_name = HSTRING::from(REGISTRY_KEY);
            let value_name = HSTRING::from(APP_NAME);
            let value_data = HSTRING::from(exe_path_str.as_ref());
            
            let mut hkey = HKEY::default();
            let result = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &key_name,
                0,
                None,
                REG_OPEN_CREATE_OPTIONS(0),
                KEY_ALL_ACCESS,
                None,
                &mut hkey,
                None,
            );
            
            if let Err(e) = result {
                return Err(anyhow::anyhow!("Failed to open registry key: {:?}", e));
            }
            
            let value_bytes = value_data.as_wide();
            let value_slice = std::slice::from_raw_parts(
                value_bytes.as_ptr() as *const u8,
                value_bytes.len() * 2,
            );
            
            let set_result = RegSetValueExW(
                hkey,
                &value_name,
                0,
                REG_SZ,
                Some(value_slice),
            );
            
            let _ = RegCloseKey(hkey);
            
            if set_result.is_ok() {
                info!("✅ Autostart enabled successfully");
                Ok(())
            } else {
                Err(anyhow::anyhow!("Failed to set registry value: {:?}", set_result))
            }
        }
    }
    
    #[cfg(not(windows))]
    {
        Err(anyhow::anyhow!("Autostart is only supported on Windows"))
    }
}

/// Disable autostart by removing registry entry
pub fn disable_autostart() -> Result<()> {
    #[cfg(windows)]
    {
        info!("Disabling autostart");
        
        unsafe {
            let key_name = HSTRING::from(REGISTRY_KEY);
            let value_name = HSTRING::from(APP_NAME);
            
            let mut hkey = HKEY::default();
            let result = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &key_name,
                0,
                None,
                REG_OPEN_CREATE_OPTIONS(0),
                KEY_ALL_ACCESS,
                None,
                &mut hkey,
                None,
            );
            
            if result.is_err() {
                warn!("Registry key not found, autostart may already be disabled");
                return Ok(());
            }
            
            let delete_result = RegDeleteValueW(hkey, &value_name);
            let _ = RegCloseKey(hkey);
            
            match delete_result {
                Ok(_) => {
                    info!("✅ Autostart disabled successfully");
                    Ok(())
                }
                Err(e) => {
                    // Check if it's just "file not found" which means it's already disabled
                    let code = e.code();
                    if code == ERROR_FILE_NOT_FOUND.to_hresult() {
                        info!("✅ Autostart was already disabled");
                        return Ok(());
                    }
                    Err(anyhow::anyhow!("Failed to delete registry value: {:?}", e))
                }
            }
        }
    }
    
    #[cfg(not(windows))]
    {
        Err(anyhow::anyhow!("Autostart is only supported on Windows"))
    }
}

/// Toggle autostart status
pub fn toggle_autostart() -> Result<bool> {
    let is_enabled = is_autostart_enabled()?;
    
    if is_enabled {
        disable_autostart()?;
        Ok(false)
    } else {
        enable_autostart()?;
        Ok(true)
    }
}

/// Get autostart status as a user-friendly string
pub fn get_autostart_status_text() -> String {
    match is_autostart_enabled() {
        Ok(true) => "✅ Autostart: Enabled".to_string(),
        Ok(false) => "⏹️ Autostart: Disabled".to_string(),
        Err(_) => "❓ Autostart: Unknown".to_string(),
    }
}