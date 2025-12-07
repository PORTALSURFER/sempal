//! Platform helpers for starting external drag-and-drop operations.
//!
//! Currently implemented for Windows by emitting a `CF_HDROP` drag with one or
//! more absolute file paths. Other platforms return an unsupported error to
//! keep behaviour predictable.

use std::path::PathBuf;

/// Start dragging the given file paths to an external target.
///
/// Returns an error if the platform does not support outgoing drags.
pub fn start_file_drag(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No files to drag".into());
    }
    platform::start_file_drag(paths)
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub fn start_file_drag(_paths: &[PathBuf]) -> Result<(), String> {
        Err("External drag-out is only supported on Windows in this build".into())
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::copy_nonoverlapping;
    use windows::Win32::Foundation::{E_INVALIDARG, POINT};
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize, DATADIR_GET, DV_E_FORMATETC,
        DVASPECT_CONTENT, FORMATETC, IDataObject, IEnumFORMATETC, STGMEDIUM, STGMEDIUM_0,
        TYMED_HGLOBAL,
    };
    use windows::Win32::System::Memory::{
        GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock, HGLOBAL,
    };
    use windows::Win32::System::Ole::{
        DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DROPEFFECT,
        DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DoDragDrop, IDropSource,
    };
    use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
    use windows::Win32::UI::Shell::{CF_HDROP, DROPFILES, SHCreateStdEnumFmtEtc};
    use windows::core::{HRESULT, implement};

    /// RAII guard to balance COM initialization.
    struct ComApartment;

    impl ComApartment {
        fn new() -> Result<Self, String> {
            // SAFETY: Single-threaded COM init for drag/drop, errors converted to string.
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                    .map_err(|err| format!("COM init failed: {err}"))?;
            }
            Ok(Self)
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    #[implement(IDataObject)]
    struct FileDropDataObject {
        paths: Vec<PathBuf>,
        format: FORMATETC,
    }

    impl FileDropDataObject {
        fn new(paths: Vec<PathBuf>) -> Result<Self, String> {
            if paths.is_empty() {
                return Err("No files to drag".into());
            }
            Ok(Self {
                paths,
                format: build_format(),
            })
        }

        fn matches_format(&self, fmt: &FORMATETC) -> bool {
            fmt.cfFormat == self.format.cfFormat
                && fmt.tymed == self.format.tymed
                && fmt.dwAspect == self.format.dwAspect
                && fmt.lindex == self.format.lindex
        }

        fn fill_medium(&self, medium: *mut STGMEDIUM) -> HRESULT {
            match create_hglobal_for_paths(&self.paths) {
                Ok(hglobal) => {
                    // SAFETY: caller provided a valid pointer.
                    unsafe {
                        *medium = STGMEDIUM {
                            tymed: TYMED_HGLOBAL,
                            Anonymous: STGMEDIUM_0 { hGlobal: hglobal },
                            pUnkForRelease: None,
                        };
                    }
                    HRESULT(0)
                }
                Err(err) => HRESULT::from_win32(err.raw_os_error().unwrap_or(1)),
            }
        }
    }

    #[allow(non_snake_case)]
    impl windows::Win32::System::Com::IDataObject_Impl for FileDropDataObject {
        fn GetData(&self, formatetcin: *const FORMATETC, medium: *mut STGMEDIUM) -> HRESULT {
            if formatetcin.is_null() || medium.is_null() {
                return E_INVALIDARG;
            }
            // SAFETY: validated against null above.
            let fmt = unsafe { &*formatetcin };
            if !self.matches_format(fmt) {
                return DV_E_FORMATETC;
            }
            self.fill_medium(medium)
        }

        fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> HRESULT {
            DV_E_FORMATETC
        }

        fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
            if pformatetc.is_null() {
                return E_INVALIDARG;
            }
            // SAFETY: validated above.
            let fmt = unsafe { &*pformatetc };
            if self.matches_format(fmt) {
                HRESULT(0)
            } else {
                DV_E_FORMATETC
            }
        }

        fn GetCanonicalFormatEtc(
            &self,
            pformatectin: *const FORMATETC,
            pformatetcout: *mut FORMATETC,
        ) -> HRESULT {
            if pformatectin.is_null() || pformatetcout.is_null() {
                return E_INVALIDARG;
            }
            // SAFETY: pointers validated above.
            unsafe {
                *pformatetcout = *pformatectin;
            }
            HRESULT(0)
        }

        fn SetData(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *const STGMEDIUM,
            _frelease: windows::Win32::Foundation::BOOL,
        ) -> HRESULT {
            windows::Win32::Foundation::E_NOTIMPL
        }

        fn EnumFormatEtc(&self, dwdirection: u32) -> windows::core::Result<IEnumFORMATETC> {
            if dwdirection == DATADIR_GET.0 {
                // SAFETY: single format descriptor provided.
                unsafe { SHCreateStdEnumFmtEtc(&[self.format.clone()]) }
            } else {
                Err(windows::Win32::Foundation::E_NOTIMPL.into())
            }
        }

        fn DAdvise(
            &self,
            _pformatetc: *const FORMATETC,
            _advf: u32,
            _padvsink: windows::core::Option<windows::Win32::System::Com::IAdviseSink>,
            _pdwconnection: *mut u32,
        ) -> HRESULT {
            windows::Win32::Foundation::E_NOTIMPL
        }

        fn DUnadvise(&self, _dwconnection: u32) -> HRESULT {
            windows::Win32::Foundation::E_NOTIMPL
        }

        fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
            Err(windows::Win32::Foundation::E_NOTIMPL.into())
        }
    }

    #[implement(IDropSource)]
    struct SimpleDropSource;

    #[allow(non_snake_case)]
    impl windows::Win32::System::Ole::IDropSource_Impl for SimpleDropSource {
        fn QueryContinueDrag(
            &self,
            escape_pressed: windows::Win32::Foundation::BOOL,
            key_state: MODIFIERKEYS_FLAGS,
        ) -> HRESULT {
            if escape_pressed.as_bool() {
                return DRAGDROP_S_CANCEL;
            }
            if key_state.0 & MK_LBUTTON.0 == 0 {
                return DRAGDROP_S_DROP;
            }
            HRESULT(0)
        }

        fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    pub fn start_file_drag(paths: &[PathBuf]) -> Result<(), String> {
        let _com = ComApartment::new()?;
        let absolute: Vec<PathBuf> = paths
            .iter()
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
            .collect();
        let data_object = FileDropDataObject::new(absolute)?;
        let drop_source = SimpleDropSource;
        // SAFETY: COM initialized above; object implementations satisfy COM contracts.
        let result = unsafe {
            DoDragDrop(
                &data_object,
                &drop_source,
                DROPEFFECT_COPY | DROPEFFECT_LINK | DROPEFFECT_MOVE,
            )
        };
        result
            .map(|_| ())
            .map_err(|err| format!("Drag failed: {err}"))
    }

    fn build_format() -> FORMATETC {
        FORMATETC {
            cfFormat: CF_HDROP.0 as u16,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL,
        }
    }

    fn create_hglobal_for_paths(paths: &[PathBuf]) -> Result<HGLOBAL, std::io::Error> {
        let mut utf16_paths = Vec::new();
        for path in paths {
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            utf16_paths.extend_from_slice(&wide);
        }
        utf16_paths.push(0); // Double null-terminator.
        let bytes_needed =
            std::mem::size_of::<DROPFILES>() + utf16_paths.len() * std::mem::size_of::<u16>();
        // SAFETY: allocating movable global memory for shell drag.
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes_needed) }
            .map_err(last_error)?;
        // SAFETY: lock global memory to populate DROPFILES header and path list.
        let ptr = unsafe { GlobalLock(handle) };
        if ptr.is_null() {
            unsafe {
                GlobalUnlock(handle);
            }
            return Err(last_error(0));
        }
        unsafe {
            let header = ptr as *mut DROPFILES;
            *header = DROPFILES {
                pFiles: std::mem::size_of::<DROPFILES>() as u32,
                pt: POINT { x: 0, y: 0 },
                fNC: false.into(),
                fWide: true.into(),
            };
            let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<DROPFILES>());
            copy_nonoverlapping(
                utf16_paths.as_ptr() as *const u8,
                data_ptr,
                utf16_paths.len() * std::mem::size_of::<u16>(),
            );
            GlobalUnlock(handle);
        }
        Ok(handle)
    }

    fn last_error(code: u32) -> std::io::Error {
        if code == 0 {
            std::io::Error::last_os_error()
        } else {
            std::io::Error::from_raw_os_error(code as i32)
        }
    }
}
