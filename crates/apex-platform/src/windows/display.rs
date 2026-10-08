//! Display modes via EnumDisplayDevicesW / EnumDisplaySettingsW (no admin needed).

use apex_core::model::DisplayInfo;
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplaySettingsW, DEVMODEW, DISPLAY_DEVICEW,
    DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_PRIMARY_DEVICE, ENUM_CURRENT_SETTINGS,
};

fn wstr(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn new_devmode() -> DEVMODEW {
    let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
    dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
    dm
}

pub fn displays() -> Vec<DisplayInfo> {
    let mut out = Vec::new();
    for i in 0..16u32 {
        let mut dd: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
        dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut dd, 0) } == 0 {
            break;
        }
        if dd.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP == 0 {
            continue;
        }
        let name_ptr = dd.DeviceName.as_ptr();
        let mut cur = new_devmode();
        if unsafe { EnumDisplaySettingsW(name_ptr, ENUM_CURRENT_SETTINGS, &mut cur) } == 0 {
            continue;
        }
        let mut max_hz = cur.dmDisplayFrequency;
        let mut mode = 0u32;
        loop {
            let mut dm = new_devmode();
            if unsafe { EnumDisplaySettingsW(name_ptr, mode, &mut dm) } == 0 {
                break;
            }
            if dm.dmPelsWidth == cur.dmPelsWidth
                && dm.dmPelsHeight == cur.dmPelsHeight
                && dm.dmDisplayFrequency > max_hz
            {
                max_hz = dm.dmDisplayFrequency;
            }
            mode += 1;
            if mode > 4096 {
                break;
            }
        }
        out.push(DisplayInfo {
            device: wstr(&dd.DeviceName),
            adapter: wstr(&dd.DeviceString),
            primary: dd.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0,
            width: cur.dmPelsWidth,
            height: cur.dmPelsHeight,
            // 0 and 1 mean "hardware default" per Win32 docs
            current_hz: if cur.dmDisplayFrequency > 1 {
                cur.dmDisplayFrequency
            } else {
                0
            },
            max_hz_at_current_resolution: if max_hz > 1 { max_hz } else { 0 },
        });
    }
    out
}

pub fn hardware_gpu_scheduling() -> Option<bool> {
    use winreg::enums::*;
    let k = winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers",
            KEY_READ,
        )
        .ok()?;
    match k.get_value::<u32, _>("HwSchMode").ok()? {
        2 => Some(true),
        1 => Some(false),
        _ => None,
    }
}
