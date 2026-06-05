//! Wi-Fi QR widget.
//!
//! Encodes Wi-Fi credentials using the de-facto-standard MECARD-style
//! string `WIFI:T:<auth>;S:<ssid>;P:<password>;H:<true|false>;;`
//! recognised by iOS and modern Android cameras. Matches
//! `addon/app/render/widgets/qr.py` byte-for-byte on the payload
//! string so a single QR image flips between the Python and Rust
//! addons without re-pairing devices.

use image::{GrayImage, Luma};
use qrcode::{Color, EcLevel, QrCode};

use crate::config::Settings;
use crate::render::fonts::{
    draw_crisp_text, draw_text, font_for, text_width, Weight, BODY_SIZE, TITLE_SIZE,
};
use crate::render::widgets::Rect;

/// MECARD-style escape: backslash, semicolon, comma, colon, double-quote.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | ';' | ',' | ':' | '"') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Build the Wi-Fi MECARD payload from user settings.
///
/// Security label normalisation: `nopass`, `open`, `none`, and the
/// empty string all collapse to `nopass` (case-insensitive). Any
/// other value is uppercased and used verbatim — `WPA`, `WEP`, etc.
pub(crate) fn payload(settings: &Settings) -> String {
    let sec_raw = settings.wifi_security.trim();
    let sec_upper = sec_raw.to_ascii_uppercase();
    let sec = match sec_upper.as_str() {
        "NOPASS" | "OPEN" | "NONE" | "" => "nopass".to_string(),
        _ => sec_upper,
    };

    let ssid = escape(&settings.wifi_ssid);
    let pwd = escape(&settings.wifi_password);
    let hidden = if settings.wifi_hidden {
        "true"
    } else {
        "false"
    };

    if sec == "nopass" {
        format!("WIFI:T:nopass;S:{ssid};H:{hidden};;")
    } else {
        format!("WIFI:T:{sec};S:{ssid};P:{pwd};H:{hidden};;")
    }
}

/// Render the Wi-Fi QR widget into `rect`.
///
/// Layout (top-to-bottom):
///
/// 1. "Wi-Fi" title at the top.
/// 2. Centred QR code square sized to fit the available height
///    after reserving room for the title and SSID caption.
/// 3. SSID caption centred below the QR code.
pub fn render(canvas: &mut GrayImage, settings: &Settings, rect: Rect) {
    // Title
    let title_f = font_for(Weight::Bold);
    draw_text(
        canvas,
        rect.x + 8,
        rect.y + 4,
        "Wi-Fi",
        title_f,
        TITLE_SIZE,
        0,
    );

    // Build QR. ERROR_CORRECT_M matches the Python addon. `.unwrap()`
    // is fine: the only failure path is "data too long for max QR
    // version" and a Wi-Fi MECARD never approaches that limit.
    let code = match QrCode::with_error_correction_level(payload(settings).as_bytes(), EcLevel::M) {
        Ok(c) => c,
        Err(_) => return,
    };
    let modules = code.width(); // QR side length in modules (excludes border)
    let bits = code.to_colors(); // row-major; Color::Dark = filled module

    // Layout: square QR sized to fit the box after reserving title +
    // caption. Border = 2 modules (matches Python `border=2`).
    let title_h: i32 = 30;
    let caption_reserve: i32 = 28;
    let available = (rect.w - 16).min(rect.h - title_h - caption_reserve);
    if available <= 0 {
        return;
    }
    let border_modules = 2;
    let total_modules = (modules as i32) + 2 * border_modules;
    // Pixel scale per module — at least 1 px to avoid degenerate output.
    let px_per_module = (available / total_modules).max(1);
    let qr_pixel_size = px_per_module * total_modules;
    let qx = rect.x + (rect.w - qr_pixel_size) / 2;
    let qy = rect.y + title_h;

    // Fill the QR background (border + inner) with white. Mostly a
    // no-op on a blank canvas but ensures a clean square if the page
    // composed something underneath.
    fill_rect(canvas, qx, qy, qr_pixel_size, qr_pixel_size, 255);

    // Draw modules. Iterate the module grid; for each dark module
    // fill its `px_per_module × px_per_module` square.
    for row in 0..modules {
        for col in 0..modules {
            let idx = row * modules + col;
            if bits[idx] != Color::Dark {
                continue;
            }
            let cell_x = qx + (col as i32 + border_modules) * px_per_module;
            let cell_y = qy + (row as i32 + border_modules) * px_per_module;
            fill_rect(canvas, cell_x, cell_y, px_per_module, px_per_module, 0);
        }
    }

    // SSID caption, centred under the QR.
    let ssid_f = font_for(Weight::Bold);
    let ssid = settings.wifi_ssid.as_str();
    let sw = text_width(ssid_f, BODY_SIZE, ssid);
    let cx = rect.x + (rect.w - sw.round() as i32) / 2;
    let cy = qy + qr_pixel_size + 4;
    draw_crisp_text(canvas, cx, cy, ssid, ssid_f, BODY_SIZE, 0);
}

/// Fill a clipped axis-aligned rectangle with a solid grayscale value.
/// Uses imageproc when fully on-canvas; otherwise hand-clips.
fn fill_rect(canvas: &mut GrayImage, x: i32, y: i32, w: i32, h: i32, fill: u8) {
    if w <= 0 || h <= 0 {
        return;
    }
    let cw = canvas.width() as i32;
    let ch = canvas.height() as i32;
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(cw);
    let y1 = (y + h).min(ch);
    for py in y0..y1 {
        for px in x0..x1 {
            canvas.put_pixel(px as u32, py as u32, Luma([fill]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with_wifi(ssid: &str, pwd: &str, sec: &str, hidden: bool) -> Settings {
        Settings {
            wifi_ssid: ssid.to_string(),
            wifi_password: pwd.to_string(),
            wifi_security: sec.to_string(),
            wifi_hidden: hidden,
            ..Settings::default()
        }
    }

    #[test]
    fn escape_passes_through_safe_chars() {
        assert_eq!(escape("HelloWorld"), "HelloWorld");
        assert_eq!(escape("1234"), "1234");
        assert_eq!(escape(""), "");
    }

    #[test]
    fn escape_backslashes_special_chars() {
        // All five MECARD specials get a leading backslash.
        assert_eq!(escape("a;b"), "a\\;b");
        assert_eq!(escape("a:b"), "a\\:b");
        assert_eq!(escape("a,b"), "a\\,b");
        assert_eq!(escape("a\"b"), "a\\\"b");
        assert_eq!(escape("a\\b"), "a\\\\b");
    }

    #[test]
    fn payload_wpa_includes_password() {
        let s = settings_with_wifi("MyNet", "secret", "WPA", false);
        assert_eq!(payload(&s), "WIFI:T:WPA;S:MyNet;P:secret;H:false;;");
    }

    #[test]
    fn payload_nopass_omits_password_field() {
        let s = settings_with_wifi("Open", "ignored", "nopass", false);
        // No `P:` field at all.
        assert_eq!(payload(&s), "WIFI:T:nopass;S:Open;H:false;;");
    }

    #[test]
    fn payload_security_aliases_collapse_to_nopass() {
        // "open", "none", and "" all become nopass (case-insensitive).
        for sec in ["open", "OPEN", "none", "None", ""] {
            let s = settings_with_wifi("X", "p", sec, false);
            assert!(
                payload(&s).contains("T:nopass"),
                "{sec:?} should map to nopass; got {}",
                payload(&s)
            );
        }
    }

    #[test]
    fn payload_hidden_flag_serialised() {
        let s = settings_with_wifi("Net", "pwd", "WPA", true);
        assert!(payload(&s).contains("H:true"));
        let s2 = settings_with_wifi("Net", "pwd", "WPA", false);
        assert!(payload(&s2).contains("H:false"));
    }

    #[test]
    fn payload_escapes_specials_in_ssid_and_password() {
        let s = settings_with_wifi("a;b", "c:d,e\"f\\g", "WPA", false);
        let p = payload(&s);
        // Each special char in ssid/password should now have a leading
        // backslash. Easiest check: every literal special char in the
        // raw inputs must appear preceded by a backslash in the output
        // (excluding the structural delimiters of the MECARD itself).
        assert!(p.contains("S:a\\;b;"), "SSID specials not escaped: {p}");
        assert!(
            p.contains("P:c\\:d\\,e\\\"f\\\\g;"),
            "PWD specials not escaped: {p}"
        );
    }

    #[test]
    fn render_writes_pixels_into_box() {
        let s = settings_with_wifi("Test", "pw", "WPA", false);
        let mut canvas = GrayImage::from_pixel(300, 300, Luma([255]));
        let rect = Rect::new(0, 0, 300, 300);
        render(&mut canvas, &s, rect);
        // QR + title should produce many dark pixels.
        let dark = canvas.pixels().filter(|p| p[0] < 200).count();
        assert!(dark > 100, "render produced only {dark} dark pixels");
    }

    #[test]
    fn render_does_not_panic_on_tiny_box() {
        let s = settings_with_wifi("Test", "pw", "WPA", false);
        let mut canvas = GrayImage::from_pixel(40, 40, Luma([255]));
        let rect = Rect::new(0, 0, 40, 40);
        render(&mut canvas, &s, rect); // just shouldn't panic
    }
}
