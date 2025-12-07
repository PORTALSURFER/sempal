//! Platform helpers for starting external drag-and-drop operations.
//!
//! Currently implemented for Windows by emitting a `CF_HDROP` drag with one or
//! more absolute file paths. Other platforms return an unsupported error to
//! keep behaviour predictable.

use std::path::PathBuf;

/// Start dragging the given file paths to an external target.
///
/// Returns an error if the platform does not support outgoing drags.
#[cfg(target_os = "windows")]
pub fn start_file_drag(hwnd: windows::Win32::Foundation::HWND, paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No files to drag".into());
    }
    platform::start_file_drag(hwnd, paths)
}

#[cfg(not(target_os = "windows"))]
pub fn start_file_drag(_hwnd: (), _paths: &[PathBuf]) -> Result<(), String> {
    Err("External drag-out is only supported on Windows in this build".into())
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::cell::Cell;
    use std::mem::ManuallyDrop;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::copy_nonoverlapping;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{DV_E_FORMATETC, E_INVALIDARG, HGLOBAL, POINT};
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows::Win32::System::Com::{
        DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IEnumFORMATETC,
        STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
    };
    use windows::Win32::System::Memory::{
        GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows::Win32::System::Ole::{
        CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE,
        OleInitialize, OleUninitialize,
    };
    use windows::Win32::UI::Shell::{DROPFILES, SHCreateStdEnumFmtEtc, SHDoDragDrop};
    use windows::core::{w, HRESULT, Ref, BOOL};
    use windows_implement::implement;

    /// RAII guard to balance COM initialization.
    struct ComApartment;

    impl ComApartment {
        fn new() -> Result<Self, String> {
            // SAFETY: Single-threaded OLE init for drag/drop, errors converted to string.
            unsafe { OleInitialize(None) }.map_err(|err| format!("COM init failed: {err}"))?;
            Ok(Self)
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe { OleUninitialize() };
        }
    }

    #[implement(IDataObject)]
    #[derive(Clone)]
    struct FileDropDataObject {
        paths: Vec<PathBuf>,
        format: FORMATETC,
        preferred_drop_effect: u16,
        performed_drop_effect: u16,
        performed_effect: Cell<DROPEFFECT>,
    }

    impl FileDropDataObject {
        fn new(paths: Vec<PathBuf>) -> Result<Self, String> {
            if paths.is_empty() {
                return Err("No files to drag".into());
            }
            let (preferred_drop_effect, performed_drop_effect) = drop_effect_formats()?;
            Ok(Self {
                paths,
                format: build_format(),
                preferred_drop_effect,
                performed_drop_effect,
                performed_effect: Cell::new(DROPEFFECT_NONE),
            })
        }

        fn matches_format(&self, fmt: &FORMATETC) -> bool {
            (fmt.cfFormat == CF_HDROP.0 as u16
                && fmt.dwAspect == DVASPECT_CONTENT.0
                && (fmt.tymed & TYMED_HGLOBAL.0 as u32) != 0
                && (fmt.lindex == -1 || fmt.lindex == 0))
                || ((fmt.cfFormat == self.preferred_drop_effect
                    || fmt.cfFormat == self.performed_drop_effect)
                    && (fmt.tymed & TYMED_HGLOBAL.0 as u32) != 0)
        }

        fn fill_medium(&self, fmt: &FORMATETC) -> windows::core::Result<STGMEDIUM> {
            if fmt.cfFormat == self.preferred_drop_effect {
                return drop_effect_medium(DROPEFFECT_COPY);
            }
            if fmt.cfFormat == self.performed_drop_effect {
                return drop_effect_medium(self.performed_effect.get());
            }
            let hglobal = create_hglobal_for_paths(&self.paths)
                .map_err(|_| windows::core::Error::from_thread())?;
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: hglobal },
                pUnkForRelease: ManuallyDrop::new(None),
            })
        }
    }

    #[allow(non_snake_case)]
    impl windows::Win32::System::Com::IDataObject_Impl for FileDropDataObject_Impl {
        fn GetData(&self, formatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
            if formatetcin.is_null() {
                return Err(windows::core::Error::from(E_INVALIDARG));
            }
            let fmt = unsafe { &*formatetcin };
            if !self.matches_format(fmt) {
                return Err(windows::core::Error::from(DV_E_FORMATETC));
            }
            self.fill_medium(fmt)
        }

        fn GetDataHere(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *mut STGMEDIUM,
        ) -> windows::core::Result<()> {
            Err(windows::core::Error::from(DV_E_FORMATETC))
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
            unsafe {
                *pformatetcout = *pformatectin;
            }
            HRESULT(0)
        }

        fn SetData(
            &self,
            pformatetc: *const FORMATETC,
            pmedium: *const STGMEDIUM,
            _frelease: BOOL,
        ) -> windows::core::Result<()> {
            if pformatetc.is_null() || pmedium.is_null() {
                return Err(windows::core::Error::from(E_INVALIDARG));
            }
            let fmt = unsafe { &*pformatetc };
            if fmt.cfFormat != self.performed_drop_effect
                || (fmt.tymed & TYMED_HGLOBAL.0 as u32) == 0
            {
                return Err(windows::core::Error::from(
                    windows::Win32::Foundation::E_NOTIMPL,
                ));
            }
            let medium = unsafe { &*pmedium };
            if medium.tymed != TYMED_HGLOBAL.0 as u32 {
                return Err(windows::core::Error::from(E_INVALIDARG));
            }
            let handle = unsafe { medium.u.hGlobal };
            let ptr = unsafe { GlobalLock(handle) } as *const u32;
            if ptr.is_null() {
                unsafe {
                    let _ = GlobalUnlock(handle);
                }
                return Err(windows::core::Error::from_thread());
            }
            let effect = unsafe { *ptr };
            self.performed_effect.set(DROPEFFECT(effect));
            unsafe {
                let _ = GlobalUnlock(handle);
            }
            Ok(())
        }

        fn EnumFormatEtc(&self, dwdirection: u32) -> windows::core::Result<IEnumFORMATETC> {
            if dwdirection != DATADIR_GET.0 as u32 {
                return Err(windows::core::Error::from(
                    windows::Win32::Foundation::E_NOTIMPL,
                ));
            }
            let formats = [
                self.format.clone(),
                build_drop_effect_format(self.preferred_drop_effect),
                build_drop_effect_format(self.performed_drop_effect),
            ];
            unsafe { SHCreateStdEnumFmtEtc(&formats) }
        }

        fn DAdvise(
            &self,
            _pformatetc: *const FORMATETC,
            _advf: u32,
            _padvsink: Ref<'_, IAdviseSink>,
        ) -> windows::core::Result<u32> {
            Err(windows::core::Error::from(
                windows::Win32::Foundation::E_NOTIMPL,
            ))
        }

        fn DUnadvise(&self, _dwconnection: u32) -> windows::core::Result<()> {
            Err(windows::core::Error::from(
                windows::Win32::Foundation::E_NOTIMPL,
            ))
        }

        fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
            Err(windows::core::Error::from(
                windows::Win32::Foundation::E_NOTIMPL,
            ))
        }
    }

    pub fn start_file_drag(hwnd: windows::Win32::Foundation::HWND, paths: &[PathBuf]) -> Result<(), String> {
        let _com = ComApartment::new()?;
        let absolute: Vec<PathBuf> = paths
            .iter()
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
            .collect();
        let data_object: IDataObject = FileDropDataObject::new(absolute)?.into();
        // SAFETY: COM initialized above; object implementations satisfy COM contracts.
        let effect = unsafe {
            SHDoDragDrop(
                Some(hwnd),
                &data_object,
                None,
                DROPEFFECT_COPY | DROPEFFECT_LINK | DROPEFFECT_MOVE,
            )
        }
        .map_err(|err| format!("Drag failed: {err}"))?;

        if effect == DROPEFFECT_NONE {
            Err("Drag canceled or target rejected drop".into())
        } else {
            Ok(())
        }
    }

    fn build_format() -> FORMATETC {
        FORMATETC {
            cfFormat: CF_HDROP.0 as u16,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    fn build_drop_effect_format(format: u16) -> FORMATETC {
        FORMATETC {
            cfFormat: format,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    fn drop_effect_formats() -> Result<(u16, u16), String> {
        static FORMATS: OnceLock<Result<(u16, u16), String>> = OnceLock::new();
        FORMATS
            .get_or_init(|| {
                let preferred = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
                let performed = unsafe { RegisterClipboardFormatW(w!("Performed DropEffect")) };
                if preferred == 0 || performed == 0 {
                    Err("RegisterClipboardFormatW failed".to_string())
                } else {
                    Ok((preferred as u16, performed as u16))
                }
            })
            .clone()
    }

    fn drop_effect_medium(effect: DROPEFFECT) -> windows::core::Result<STGMEDIUM> {
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, std::mem::size_of::<u32>()) }
            .map_err(|_| windows::core::Error::from_thread())?;
        let ptr = unsafe { GlobalLock(handle) } as *mut u32;
        if ptr.is_null() {
            unsafe {
                let _ = GlobalUnlock(handle);
            }
            return Err(windows::core::Error::from_thread());
        }
        unsafe {
            *ptr = effect.0;
            let _ = GlobalUnlock(handle);
        }
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: handle },
            pUnkForRelease: ManuallyDrop::new(None),
        })
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
            .map_err(last_error_from_win32)?;
        // SAFETY: lock global memory to populate DROPFILES header and path list.
        let ptr = unsafe { GlobalLock(handle) };
        if ptr.is_null() {
            unsafe {
                let _ = GlobalUnlock(handle);
            }
            return Err(std::io::Error::last_os_error());
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
            let _ = GlobalUnlock(handle);
        }
        Ok(handle)
    }

    fn last_error_from_win32(err: windows::core::Error) -> std::io::Error {
        std::io::Error::from_raw_os_error(err.code().0)
    }
}
