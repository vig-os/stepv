//! C ABI over `stepv`'s renderer and header reader (`stepv.h`).
//!
//! For the macOS Quick Look extensions: their sandbox forbids exec, so they
//! cannot run the `stepv` CLI and link this instead, next to the in-process
//! kernel (`kernel/stepv_occt.h`). Thumbnails stay pixel-identical to the
//! CLI's, overlay included, because this calls the same code.
//!
//! No panic may unwind into Swift: every entry point catches it and reports
//! `STEPV_ERR_INTERNAL`.

use std::ffi::{CStr, CString, c_char};
use std::panic::catch_unwind;
use std::path::Path;

use stepv::{header, occt, render};

pub const STEPV_OK: i32 = 0;
pub const STEPV_ERR_ARGS: i32 = 1;
pub const STEPV_ERR_DECODE: i32 = 2;
pub const STEPV_ERR_EMPTY: i32 = 3;
pub const STEPV_ERR_INTERNAL: i32 = 4;

/// See `stepv.h`.
///
/// # Safety
/// `mesh` must point to `mesh_len` readable bytes; `out` and `out_len` must be
/// valid for writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stepv_render_png(
    mesh: *const u8,
    mesh_len: usize,
    size: u32,
    show_construction: bool,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    if mesh.is_null() || out.is_null() || out_len.is_null() || !(16..=4096).contains(&size) {
        return STEPV_ERR_ARGS;
    }
    // SAFETY: caller contract above.
    let bytes = unsafe { std::slice::from_raw_parts(mesh, mesh_len) };
    let result = catch_unwind(|| {
        let scene = occt::read_mesh(bytes).map_err(|_| STEPV_ERR_DECODE)?;
        let opts = render::Options {
            show_construction,
            ..render::Options::square(size)
        };
        let img = render::render(&scene, &opts).map_err(|_| STEPV_ERR_EMPTY)?;
        img.to_png().map_err(|_| STEPV_ERR_INTERNAL)
    });
    match result {
        Ok(Ok(png)) => {
            let boxed = png.into_boxed_slice();
            // SAFETY: caller contract above.
            unsafe {
                *out_len = boxed.len();
                *out = Box::into_raw(boxed).cast::<u8>();
            }
            STEPV_OK
        }
        Ok(Err(code)) => code,
        Err(_) => STEPV_ERR_INTERNAL,
    }
}

/// See `stepv.h`.
///
/// # Safety
/// `p`/`len` must come from `stepv_render_png`, and not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stepv_buffer_free(p: *mut u8, len: usize) {
    if !p.is_null() {
        // SAFETY: reconstructs the Box<[u8]> stepv_render_png leaked.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, len)) });
    }
}

/// See `stepv.h`.
///
/// # Safety
/// `path` must be a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stepv_info_json(path: *const c_char) -> *mut c_char {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller contract above.
    let path = unsafe { CStr::from_ptr(path) }
        .to_string_lossy()
        .into_owned();
    let json = catch_unwind(|| serde_json_string(&header::read(Path::new(&path))))
        .unwrap_or_else(|_| r#"{"header_error":"internal error"}"#.to_owned());
    CString::new(json).map_or(std::ptr::null_mut(), CString::into_raw)
}

fn serde_json_string(info: &header::Info) -> String {
    // Info is plain data; serialising it cannot fail.
    stepv::header::to_json(info)
}

/// See `stepv.h`.
///
/// # Safety
/// `p` must come from `stepv_info_json`, and not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stepv_string_free(p: *mut c_char) {
    if !p.is_null() {
        // SAFETY: reconstructs the CString stepv_info_json leaked.
        drop(unsafe { CString::from_raw(p) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-triangle STEPVMSH v3 file, as the kernel writes it.
    fn mesh() -> Vec<u8> {
        let mut v = b"STEPVMSH".to_vec();
        v.extend(3u32.to_le_bytes());
        for x in [0.0f64, 0.0, 0.0, 1.0, 1.0, 0.0] {
            v.extend(x.to_le_bytes());
        }
        v.extend(1u32.to_le_bytes()); // parts
        v.extend(0u32.to_le_bytes()); // name
        v.push(1);
        for x in [0.2f32, 0.4, 0.8] {
            v.extend(x.to_le_bytes());
        }
        v.extend(1u32.to_le_bytes()); // faces
        v.extend([0u8, 0]);
        v.extend([0u8; 12]);
        v.extend(3u32.to_le_bytes());
        v.extend(1u32.to_le_bytes());
        for x in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            v.extend(x.to_le_bytes());
        }
        for x in [0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0] {
            v.extend(x.to_le_bytes());
        }
        for i in [0u32, 1, 2, 0] {
            v.extend(i.to_le_bytes());
        }
        v.extend(0u32.to_le_bytes()); // segments
        v
    }

    #[test]
    fn renders_a_png_and_frees_it() {
        let m = mesh();
        let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
        let rc =
            unsafe { stepv_render_png(m.as_ptr(), m.len(), 64, false, &raw mut out, &raw mut len) };
        assert_eq!(rc, STEPV_OK);
        let png = unsafe { std::slice::from_raw_parts(out, len) };
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        unsafe { stepv_buffer_free(out, len) };
    }

    #[test]
    fn rejects_bad_input_without_panicking() {
        let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
        let junk = [0u8; 10];
        assert_eq!(
            unsafe {
                stepv_render_png(
                    junk.as_ptr(),
                    junk.len(),
                    64,
                    false,
                    &raw mut out,
                    &raw mut len,
                )
            },
            STEPV_ERR_DECODE
        );
        let m = mesh();
        assert_eq!(
            unsafe { stepv_render_png(m.as_ptr(), m.len(), 8, false, &raw mut out, &raw mut len) },
            STEPV_ERR_ARGS
        );
        assert_eq!(
            unsafe { stepv_render_png(std::ptr::null(), 0, 64, false, &raw mut out, &raw mut len) },
            STEPV_ERR_ARGS
        );
        assert!(out.is_null());
    }

    #[test]
    fn info_json_never_fails() {
        let p = CString::new("/definitely/not/here.step").unwrap();
        let s = unsafe { stepv_info_json(p.as_ptr()) };
        assert!(!s.is_null());
        let json = unsafe { CStr::from_ptr(s) }.to_str().unwrap().to_owned();
        unsafe { stepv_string_free(s) };
        let v: serde_json_value::Value = serde_json_value::from_str(&json);
        assert!(v.header_error.is_some());
    }

    /// Minimal stand-in so the test does not need serde_json here.
    mod serde_json_value {
        pub struct Value {
            pub header_error: Option<String>,
        }
        pub fn from_str(s: &str) -> Value {
            let has = s.contains("\"header_error\":\"");
            Value {
                header_error: has.then(|| "set".to_owned()),
            }
        }
    }
}
