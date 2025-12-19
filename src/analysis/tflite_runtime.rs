use std::ffi::{CString, c_char, c_int, c_void};
use std::path::Path;

use libloading::{Library, Symbol};

#[repr(C)]
struct TfLiteModel;
#[repr(C)]
struct TfLiteInterpreterOptions;
#[repr(C)]
struct TfLiteInterpreter;
#[repr(C)]
struct TfLiteTensor;

type TfLiteStatus = c_int;
type TfLiteType = c_int;

const TFLITE_OK: TfLiteStatus = 0;
const TFLITE_FLOAT32: TfLiteType = 1;
const EMBEDDING_DIM: usize = 1024;

pub(crate) struct TfliteRuntime {
    api: TfliteApi,
    model: *mut TfLiteModel,
    options: *mut TfLiteInterpreterOptions,
    interpreter: *mut TfLiteInterpreter,
}

impl TfliteRuntime {
    pub(crate) fn load(model_path: &Path, lib_path: &Path, threads: i32) -> Result<Self, String> {
        let api = TfliteApi::load(lib_path)?;
        let model_c = CString::new(model_path.to_string_lossy().as_bytes())
            .map_err(|_| "Model path contains null bytes".to_string())?;
        let model = unsafe { (api.model_create)(model_c.as_ptr()) };
        if model.is_null() {
            return Err(format!(
                "Failed to load TFLite model at {}",
                model_path.to_string_lossy()
            ));
        }
        let options = unsafe { (api.interpreter_options_create)() };
        if options.is_null() {
            unsafe { (api.model_delete)(model) };
            return Err("Failed to create TFLite interpreter options".to_string());
        }
        unsafe { (api.interpreter_options_set_num_threads)(options, threads) };
        let interpreter = unsafe { (api.interpreter_create)(model, options) };
        if interpreter.is_null() {
            unsafe { (api.interpreter_options_delete)(options) };
            unsafe { (api.model_delete)(model) };
            return Err("Failed to create TFLite interpreter".to_string());
        }
        let status = unsafe { (api.interpreter_allocate_tensors)(interpreter) };
        if status != TFLITE_OK {
            unsafe { (api.interpreter_delete)(interpreter) };
            unsafe { (api.interpreter_options_delete)(options) };
            unsafe { (api.model_delete)(model) };
            return Err("Failed to allocate TFLite tensors".to_string());
        }
        Ok(Self {
            api,
            model,
            options,
            interpreter,
        })
    }

    pub(crate) fn run(&mut self, input: &[f32]) -> Result<Vec<f32>, String> {
        let input_tensor =
            unsafe { (self.api.interpreter_get_input_tensor)(self.interpreter, 0) };
        if input_tensor.is_null() {
            return Err("Failed to get TFLite input tensor".to_string());
        }
        let tensor_type = unsafe { (self.api.tensor_type)(input_tensor) };
        if tensor_type != TFLITE_FLOAT32 {
            return Err(format!("Unexpected input tensor type {tensor_type}"));
        }
        let byte_size = unsafe { (self.api.tensor_byte_size)(input_tensor) };
        let expected_bytes = (input.len() * std::mem::size_of::<f32>()) as usize;
        if byte_size < expected_bytes {
            return Err(format!(
                "Input tensor too small: {byte_size} bytes for {expected_bytes}"
            ));
        }
        let status = unsafe {
            (self.api.tensor_copy_from_buffer)(
                input_tensor,
                input.as_ptr() as *const c_void,
                expected_bytes,
            )
        };
        if status != TFLITE_OK {
            return Err("Failed to copy input tensor data".to_string());
        }
        let status = unsafe { (self.api.interpreter_invoke)(self.interpreter) };
        if status != TFLITE_OK {
            return Err("Failed to invoke TFLite interpreter".to_string());
        }
        self.read_embedding()
    }

    fn read_embedding(&self) -> Result<Vec<f32>, String> {
        let output_count = unsafe { (self.api.interpreter_get_output_tensor_count)(self.interpreter) };
        if output_count <= 0 {
            return Err("No TFLite outputs available".to_string());
        }
        for index in 0..output_count {
            let tensor = unsafe { (self.api.interpreter_get_output_tensor)(self.interpreter, index) };
            if tensor.is_null() {
                continue;
            }
            let tensor_type = unsafe { (self.api.tensor_type)(tensor) };
            if tensor_type != TFLITE_FLOAT32 {
                continue;
            }
            let dims = tensor_dims(&self.api, tensor);
            if dims.is_empty() {
                continue;
            }
            if *dims.last().unwrap_or(&0) != EMBEDDING_DIM as i32 {
                continue;
            }
            let element_count: usize = dims
                .iter()
                .copied()
                .map(|v| v.max(1) as usize)
                .product();
            if element_count == 0 {
                continue;
            }
            let mut data = vec![0.0_f32; element_count];
            let byte_size = data.len() * std::mem::size_of::<f32>();
            let status = unsafe {
                (self.api.tensor_copy_to_buffer)(
                    tensor,
                    data.as_mut_ptr() as *mut c_void,
                    byte_size,
                )
            };
            if status != TFLITE_OK {
                return Err("Failed to read TFLite output tensor".to_string());
            }
            let frame_count = element_count / EMBEDDING_DIM;
            if frame_count <= 1 {
                return Ok(data);
            }
            let mut pooled = vec![0.0_f32; EMBEDDING_DIM];
            for frame in 0..frame_count {
                let base = frame * EMBEDDING_DIM;
                let chunk = &data[base..base + EMBEDDING_DIM];
                for (idx, value) in chunk.iter().enumerate() {
                    pooled[idx] += *value;
                }
            }
            for value in &mut pooled {
                *value /= frame_count as f32;
            }
            return Ok(pooled);
        }
        Err("No embedding output found in TFLite outputs".to_string())
    }
}

impl Drop for TfliteRuntime {
    fn drop(&mut self) {
        unsafe {
            (self.api.interpreter_delete)(self.interpreter);
            (self.api.interpreter_options_delete)(self.options);
            (self.api.model_delete)(self.model);
        }
    }
}

fn tensor_dims(api: &TfliteApi, tensor: *const TfLiteTensor) -> Vec<i32> {
    let dims = unsafe { (api.tensor_num_dims)(tensor) };
    if dims <= 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(dims as usize);
    for i in 0..dims {
        let dim = unsafe { (api.tensor_dim)(tensor, i) };
        out.push(dim);
    }
    out
}

struct TfliteApi {
    _lib: Library,
    model_create: Symbol<'static, unsafe extern "C" fn(*const c_char) -> *mut TfLiteModel>,
    model_delete: Symbol<'static, unsafe extern "C" fn(*mut TfLiteModel)>,
    interpreter_options_create: Symbol<'static, unsafe extern "C" fn() -> *mut TfLiteInterpreterOptions>,
    interpreter_options_delete: Symbol<'static, unsafe extern "C" fn(*mut TfLiteInterpreterOptions)>,
    interpreter_options_set_num_threads:
        Symbol<'static, unsafe extern "C" fn(*mut TfLiteInterpreterOptions, c_int)>,
    interpreter_create:
        Symbol<'static, unsafe extern "C" fn(*const TfLiteModel, *const TfLiteInterpreterOptions) -> *mut TfLiteInterpreter>,
    interpreter_delete: Symbol<'static, unsafe extern "C" fn(*mut TfLiteInterpreter)>,
    interpreter_allocate_tensors: Symbol<'static, unsafe extern "C" fn(*mut TfLiteInterpreter) -> TfLiteStatus>,
    interpreter_get_input_tensor:
        Symbol<'static, unsafe extern "C" fn(*const TfLiteInterpreter, c_int) -> *mut TfLiteTensor>,
    interpreter_get_output_tensor:
        Symbol<'static, unsafe extern "C" fn(*const TfLiteInterpreter, c_int) -> *const TfLiteTensor>,
    interpreter_get_output_tensor_count:
        Symbol<'static, unsafe extern "C" fn(*const TfLiteInterpreter) -> c_int>,
    interpreter_invoke: Symbol<'static, unsafe extern "C" fn(*mut TfLiteInterpreter) -> TfLiteStatus>,
    tensor_copy_from_buffer:
        Symbol<'static, unsafe extern "C" fn(*mut TfLiteTensor, *const c_void, usize) -> TfLiteStatus>,
    tensor_copy_to_buffer:
        Symbol<'static, unsafe extern "C" fn(*const TfLiteTensor, *mut c_void, usize) -> TfLiteStatus>,
    tensor_type: Symbol<'static, unsafe extern "C" fn(*const TfLiteTensor) -> TfLiteType>,
    tensor_num_dims: Symbol<'static, unsafe extern "C" fn(*const TfLiteTensor) -> c_int>,
    tensor_dim: Symbol<'static, unsafe extern "C" fn(*const TfLiteTensor, c_int) -> c_int>,
    tensor_byte_size: Symbol<'static, unsafe extern "C" fn(*const TfLiteTensor) -> usize>,
}

impl TfliteApi {
    fn load(path: &Path) -> Result<Self, String> {
        let lib = unsafe { Library::new(path) }
            .map_err(|err| format!("Failed to load TFLite runtime at {}: {err}", path.display()))?;
        unsafe {
            let api = TfliteApi {
                model_create: lib.get(b"TfLiteModelCreateFromFile\0")?.into_raw(),
                model_delete: lib.get(b"TfLiteModelDelete\0")?.into_raw(),
                interpreter_options_create: lib
                    .get(b"TfLiteInterpreterOptionsCreate\0")?
                    .into_raw(),
                interpreter_options_delete: lib
                    .get(b"TfLiteInterpreterOptionsDelete\0")?
                    .into_raw(),
                interpreter_options_set_num_threads: lib
                    .get(b"TfLiteInterpreterOptionsSetNumThreads\0")?
                    .into_raw(),
                interpreter_create: lib.get(b"TfLiteInterpreterCreate\0")?.into_raw(),
                interpreter_delete: lib.get(b"TfLiteInterpreterDelete\0")?.into_raw(),
                interpreter_allocate_tensors: lib
                    .get(b"TfLiteInterpreterAllocateTensors\0")?
                    .into_raw(),
                interpreter_get_input_tensor: lib
                    .get(b"TfLiteInterpreterGetInputTensor\0")?
                    .into_raw(),
                interpreter_get_output_tensor: lib
                    .get(b"TfLiteInterpreterGetOutputTensor\0")?
                    .into_raw(),
                interpreter_get_output_tensor_count: lib
                    .get(b"TfLiteInterpreterGetOutputTensorCount\0")?
                    .into_raw(),
                interpreter_invoke: lib.get(b"TfLiteInterpreterInvoke\0")?.into_raw(),
                tensor_copy_from_buffer: lib.get(b"TfLiteTensorCopyFromBuffer\0")?.into_raw(),
                tensor_copy_to_buffer: lib.get(b"TfLiteTensorCopyToBuffer\0")?.into_raw(),
                tensor_type: lib.get(b"TfLiteTensorType\0")?.into_raw(),
                tensor_num_dims: lib.get(b"TfLiteTensorNumDims\0")?.into_raw(),
                tensor_dim: lib.get(b"TfLiteTensorDim\0")?.into_raw(),
                tensor_byte_size: lib.get(b"TfLiteTensorByteSize\0")?.into_raw(),
                _lib: lib,
            };
            Ok(api)
        }
        .map_err(|err: libloading::Error| format!("Failed to load TFLite symbols: {err}"))
    }
}
