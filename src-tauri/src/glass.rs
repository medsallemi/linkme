use tauri::{AppHandle, Manager, Runtime, Theme, WebviewWindowBuilder, window::Color};

use crate::models::{CaptureSettings, MAX_CAPTURE_HIGHLIGHT, MAX_PANEL_TRANSPARENCY};

pub fn material_allowed() -> bool {
    #[cfg(windows)]
    {
        windows_material::allowed()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn appearance_script(settings: &CaptureSettings) -> String {
    let highlight = settings.highlight.min(MAX_CAPTURE_HIGHLIGHT);
    let transparency = settings.panel_transparency.min(MAX_PANEL_TRANSPARENCY);
    format!(
        "document.documentElement.dataset.captureHighlight='{highlight}';document.documentElement.style.setProperty('--capture-transparency','{transparency}%');"
    )
}

pub fn popup<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    app: &AppHandle<R>,
    settings: &CaptureSettings,
    generation: u32,
) -> WebviewWindowBuilder<'a, R, M> {
    let appearance = appearance_script(settings);
    let acrylic = material_allowed();
    let dark = app
        .get_webview_window("main")
        .and_then(|window| window.theme().ok())
        == Some(Theme::Dark);
    // Match the solid --shell fallback in glass.css. Zero alpha lets DWM show through.
    let tone = if dark { 0x20 } else { 0xf4 };
    builder
        .transparent(acrylic)
        .background_color(Color(tone, tone, tone, if acrylic { 0 } else { 255 }))
        .initialization_script(format!(
            "document.addEventListener('DOMContentLoaded', () => {{ {appearance} }}, {{once:true}})"
        ))
        .on_page_load(move |window, event| {
            if event.event() != tauri::webview::PageLoadEvent::Finished {
                return;
            }
            #[cfg(windows)]
            let applied = acrylic && window.hwnd().is_ok_and(windows_material::apply);
            #[cfg(not(windows))]
            let applied = false;
            let _ = window.eval(format!(
                "document.documentElement.classList.toggle('native-acrylic',{applied});\
                 document.documentElement.dataset.popupGeneration='{generation}';\
                 window.dispatchEvent(new Event('linkme-popup-material-ready'));"
            ));
        })
}

#[cfg(windows)]
pub(crate) mod windows_material {
    use std::{ffi::c_void, mem::size_of};
    use windows::{
        Win32::{
            Foundation::HWND,
            Graphics::Dwm::{
                DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DwmExtendFrameIntoClientArea,
                DwmSetWindowAttribute,
            },
            System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
            UI::{
                Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
                Controls::MARGINS,
                WindowsAndMessaging::{
                    SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
                },
            },
        },
        core::w,
    };

    pub fn allowed() -> bool {
        let mut contrast = HIGHCONTRASTW {
            cbSize: size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        let mut transparency = 0u32;
        let mut bytes = size_of::<u32>() as u32;
        // These read-only checks target one Windows appearance preference and its accessibility override.
        unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                Some((&mut contrast as *mut HIGHCONTRASTW).cast::<c_void>()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .is_ok()
                && contrast.dwFlags.0 & HCF_HIGHCONTRASTON.0 == 0
                && RegGetValueW(
                    HKEY_CURRENT_USER,
                    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                    w!("EnableTransparency"),
                    RRF_RT_REG_DWORD,
                    None,
                    Some((&mut transparency as *mut u32).cast::<c_void>()),
                    Some(&mut bytes),
                )
                .is_ok()
                && transparency == 1
        }
    }

    pub fn apply(window: HWND) -> bool {
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        // The DWM calls are synchronous and the window comes from a live Tauri popup.
        unsafe {
            DwmSetWindowAttribute(
                window,
                DWMWA_SYSTEMBACKDROP_TYPE,
                std::ptr::from_ref(&DWMSBT_TRANSIENTWINDOW).cast(),
                size_of_val(&DWMSBT_TRANSIENTWINDOW) as u32,
            )
            .is_ok()
                && DwmExtendFrameIntoClientArea(window, &margins).is_ok()
        }
    }
}
