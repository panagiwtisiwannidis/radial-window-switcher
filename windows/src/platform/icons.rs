use std::ffi::c_void;

use tiny_skia::Pixmap;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetDIBits, GetObjectW,
    ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::UI::Shell::SHDefExtractIconW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, GetClassLongPtrW, GetIconInfo, SendMessageTimeoutW, GCLP_HICON, HICON,
    ICONINFO, ICON_BIG, SMTO_ABORTIFHUNG, WM_GETICON,
};

use super::util::wide;

const ICON_SIZE: u32 = 64;

pub fn for_executable(path: &str) -> Option<Pixmap> {
    let path = wide(path);
    let mut large = HICON::default();
    unsafe {
        SHDefExtractIconW(PCWSTR(path.as_ptr()), 0, 0, Some(&mut large), None, (16 << 16) | ICON_SIZE).ok().ok()?;
        if large.is_invalid() {
            return None;
        }
        let pixmap = to_pixmap(large);
        let _ = DestroyIcon(large);
        pixmap
    }
}

pub fn for_window(hwnd: HWND) -> Option<Pixmap> {
    unsafe {
        let mut result = 0usize;
        SendMessageTimeoutW(hwnd, WM_GETICON, WPARAM(ICON_BIG as usize), LPARAM(0), SMTO_ABORTIFHUNG, 50, Some(&mut result));
        if result == 0 {
            result = GetClassLongPtrW(hwnd, GCLP_HICON);
        }
        if result == 0 {
            return None;
        }
        to_pixmap(HICON(result as *mut c_void))
    }
}

unsafe fn bitmap_pixels(bitmap: HBITMAP, width: i32, height: i32) -> Option<Vec<u8>> {
    let dc = CreateCompatibleDC(None);
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let lines = GetDIBits(dc, bitmap, 0, height as u32, Some(pixels.as_mut_ptr() as *mut c_void), &mut info, DIB_RGB_COLORS);
    let _ = DeleteDC(dc);
    (lines != 0).then_some(pixels)
}

pub unsafe fn to_pixmap(icon: HICON) -> Option<Pixmap> {
    let mut info = ICONINFO::default();
    GetIconInfo(icon, &mut info).ok()?;
    let release = |info: &ICONINFO| {
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(info.hbmColor);
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(info.hbmMask);
        }
    };
    if info.hbmColor.is_invalid() {
        release(&info);
        return None;
    }
    let mut bitmap = BITMAP::default();
    GetObjectW(info.hbmColor, std::mem::size_of::<BITMAP>() as i32, Some(&mut bitmap as *mut _ as *mut c_void));
    let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);
    let color = bitmap_pixels(info.hbmColor, width, height);
    let has_alpha = color.as_ref().is_some_and(|c| c.chunks(4).any(|p| p[3] != 0));
    let mask = if has_alpha { None } else { bitmap_pixels(info.hbmMask, width, height) };
    release(&info);

    let color = color?;
    let mut pixmap = Pixmap::new(width as u32, height as u32)?;
    for (i, (src, dst)) in color.chunks(4).zip(pixmap.data_mut().chunks_mut(4)).enumerate() {
        let alpha = match &mask {
            Some(mask) => 255 - mask[i * 4],
            None => src[3],
        } as u32;
        dst[0] = (src[2] as u32 * alpha / 255) as u8;
        dst[1] = (src[1] as u32 * alpha / 255) as u8;
        dst[2] = (src[0] as u32 * alpha / 255) as u8;
        dst[3] = alpha as u8;
    }
    Some(pixmap)
}

pub fn to_hicon(pixmap: &Pixmap) -> Option<HICON> {
    let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
    unsafe {
        let screen = GetDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let color = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, None, 0);
        ReleaseDC(None, screen);
        let color = color.ok()?;
        let target = std::slice::from_raw_parts_mut(bits as *mut u8, (width * height * 4) as usize);
        for (src, dst) in pixmap.data().chunks(4).zip(target.chunks_mut(4)) {
            let a = src[3] as u32;
            let straight = |c: u8| if a == 0 { 0 } else { (c as u32 * 255 / a).min(255) as u8 };
            dst[0] = straight(src[2]);
            dst[1] = straight(src[1]);
            dst[2] = straight(src[0]);
            dst[3] = src[3];
        }
        let mask = CreateBitmap(width, height, 1, 1, None);
        let icon = CreateIconIndirect(&ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color });
        let _ = DeleteObject(color);
        let _ = DeleteObject(mask);
        icon.ok()
    }
}
