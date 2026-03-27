#![cfg(feature = "opencl")]

use opencl3 as ocl;
use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;
#[cfg(feature = "cuda")]
use std::sync::Mutex;

#[cfg(feature = "cuda")]
use cudarc::driver::{
    CudaContext, CudaFunction, CudaModule, CudaSlice, CudaStream, DeviceRepr, LaunchConfig,
    PushKernelArg,
};
#[cfg(feature = "cuda")]
use cudarc::nvrtc::compile_ptx;

#[allow(non_camel_case_types)]
pub type cl_device_id = ocl::types::cl_device_id;
#[allow(non_camel_case_types)]
pub type cl_device_type = ocl::types::cl_device_type;

pub const CL_DEVICE_TYPE_GPU: cl_device_type = ocl::device::CL_DEVICE_TYPE_GPU;
pub const CL_DEVICE_TYPE_CPU: cl_device_type = ocl::device::CL_DEVICE_TYPE_CPU;
pub const CL_MEM_READ_ONLY: ocl::types::cl_mem_flags = ocl::memory::CL_MEM_READ_ONLY;
pub const CL_MEM_READ_WRITE: ocl::types::cl_mem_flags = ocl::memory::CL_MEM_READ_WRITE;
pub const CL_TRUE: ocl::types::cl_bool = ocl::types::CL_TRUE;
pub const CL_INVALID_VALUE: i32 = ocl::error_codes::CL_INVALID_VALUE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClError(pub i32);

impl std::fmt::Display for ClError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClError({})", self.0)
    }
}

impl std::error::Error for ClError {}

impl From<ocl::error_codes::ClError> for ClError {
    fn from(value: ocl::error_codes::ClError) -> Self {
        Self(value.0)
    }
}

impl From<i32> for ClError {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl From<String> for ClError {
    fn from(_value: String) -> Self {
        Self(-1)
    }
}

#[cfg(feature = "cuda")]
impl From<cudarc::driver::DriverError> for ClError {
    fn from(_value: cudarc::driver::DriverError) -> Self {
        Self(-1)
    }
}

#[cfg(feature = "cuda")]
impl From<cudarc::nvrtc::CompileError> for ClError {
    fn from(_value: cudarc::nvrtc::CompileError) -> Self {
        Self(-1)
    }
}

pub type Result<T> = std::result::Result<T, ClError>;

pub trait GpuData: Copy + Default + Send + Sync + 'static {}
impl GpuData for i8 {}
impl GpuData for i32 {}
impl GpuData for f32 {}
impl GpuData for f64 {}
impl GpuData for [f32; 4] {}

#[derive(Clone)]
pub struct Device {
    backend: DeviceBackend,
}

#[derive(Clone)]
enum DeviceBackend {
    OpenCl(ocl::device::Device),
    #[cfg(feature = "cuda")]
    Cuda {
        ordinal: usize,
    },
}

impl Device {
    pub fn new(id: cl_device_id) -> Self {
        Self {
            backend: DeviceBackend::OpenCl(ocl::device::Device::new(id)),
        }
    }

    #[cfg(feature = "cuda")]
    pub fn cuda(ordinal: usize) -> Self {
        Self {
            backend: DeviceBackend::Cuda { ordinal },
        }
    }

    pub fn id(&self) -> cl_device_id {
        match &self.backend {
            DeviceBackend::OpenCl(d) => d.id(),
            #[cfg(feature = "cuda")]
            DeviceBackend::Cuda { .. } => ptr::null_mut(),
        }
    }

    pub fn name(&self) -> Result<String> {
        match &self.backend {
            DeviceBackend::OpenCl(d) => d.name().map_err(Into::into),
            #[cfg(feature = "cuda")]
            DeviceBackend::Cuda { ordinal } => Ok(format!("CUDA Device {}", ordinal)),
        }
    }

    pub fn vendor(&self) -> Result<String> {
        match &self.backend {
            DeviceBackend::OpenCl(d) => d.vendor().map_err(Into::into),
            #[cfg(feature = "cuda")]
            DeviceBackend::Cuda { .. } => Ok("NVIDIA".to_string()),
        }
    }

    pub fn dev_type(&self) -> Result<cl_device_type> {
        match &self.backend {
            DeviceBackend::OpenCl(d) => d.dev_type().map_err(Into::into),
            #[cfg(feature = "cuda")]
            DeviceBackend::Cuda { .. } => Ok(CL_DEVICE_TYPE_GPU),
        }
    }

    pub fn is_cuda(&self) -> bool {
        match self.backend {
            DeviceBackend::OpenCl(_) => false,
            #[cfg(feature = "cuda")]
            DeviceBackend::Cuda { .. } => true,
        }
    }
}

pub struct Context {
    backend: ContextBackend,
}

enum ContextBackend {
    OpenCl(ocl::context::Context),
    #[cfg(feature = "cuda")]
    Cuda {
        ctx: Arc<CudaContext>,
        stream: Arc<CudaStream>,
    },
}

impl Context {
    pub fn from_device(device: &Device) -> Result<Self> {
        match &device.backend {
            DeviceBackend::OpenCl(d) => {
                let ctx = ocl::context::Context::from_device(d).map_err(ClError::from)?;
                Ok(Self {
                    backend: ContextBackend::OpenCl(ctx),
                })
            }
            #[cfg(feature = "cuda")]
            DeviceBackend::Cuda { ordinal } => {
                let ctx = CudaContext::new(*ordinal).map_err(ClError::from)?;
                let stream = ctx.default_stream();
                Ok(Self {
                    backend: ContextBackend::Cuda { ctx, stream },
                })
            }
            #[cfg(not(feature = "cuda"))]
            _ => Err(ClError(-1)),
        }
    }

    pub fn is_cuda(&self) -> bool {
        match self.backend {
            ContextBackend::OpenCl(_) => false,
            #[cfg(feature = "cuda")]
            ContextBackend::Cuda { .. } => true,
        }
    }

    fn opencl(&self) -> Option<&ocl::context::Context> {
        match &self.backend {
            ContextBackend::OpenCl(ctx) => Some(ctx),
            #[cfg(feature = "cuda")]
            ContextBackend::Cuda { .. } => None,
        }
    }

    #[cfg(feature = "cuda")]
    fn cuda_ctx(&self) -> Option<&Arc<CudaContext>> {
        match &self.backend {
            ContextBackend::OpenCl(_) => None,
            ContextBackend::Cuda { ctx, .. } => Some(ctx),
        }
    }

    #[cfg(feature = "cuda")]
    fn cuda_stream(&self) -> Option<&Arc<CudaStream>> {
        match &self.backend {
            ContextBackend::OpenCl(_) => None,
            ContextBackend::Cuda { stream, .. } => Some(stream),
        }
    }
}

#[derive(Clone)]
pub struct CommandQueue {
    backend: CommandQueueBackend,
}

#[derive(Clone)]
enum CommandQueueBackend {
    OpenCl(Arc<ocl::command_queue::CommandQueue>),
    #[cfg(feature = "cuda")]
    Cuda(Arc<CudaStream>),
}

impl CommandQueue {
    pub unsafe fn create_with_properties(
        context: &Context,
        device_id: cl_device_id,
        properties: ocl::types::cl_command_queue_properties,
        queue_size: ocl::types::cl_uint,
    ) -> Result<Self> {
        if let Some(ctx) = context.opencl() {
            let q = ocl::command_queue::CommandQueue::create_with_properties(
                ctx, device_id, properties, queue_size,
            )
            .map_err(ClError::from)?;
            return Ok(Self {
                backend: CommandQueueBackend::OpenCl(Arc::new(q)),
            });
        }
        #[cfg(feature = "cuda")]
        {
            if let Some(stream) = context.cuda_stream() {
                return Ok(Self {
                    backend: CommandQueueBackend::Cuda(stream.clone()),
                });
            }
        }
        Err(ClError(CL_INVALID_VALUE))
    }

    pub fn finish(&self) -> Result<()> {
        match &self.backend {
            CommandQueueBackend::OpenCl(q) => q.finish().map_err(Into::into),
            #[cfg(feature = "cuda")]
            CommandQueueBackend::Cuda(stream) => stream.synchronize().map_err(Into::into),
        }
    }

    pub unsafe fn enqueue_write_buffer<T: GpuData + CudaData>(
        &self,
        buffer: &mut Buffer<T>,
        _blocking_write: ocl::types::cl_bool,
        offset: usize,
        data: &[T],
        _event_wait_list: &[ocl::types::cl_event],
    ) -> Result<()> {
        match (&self.backend, &mut buffer.backend) {
            (CommandQueueBackend::OpenCl(q), BufferBackend::OpenCl(buf)) => {
                q.enqueue_write_buffer(buf, CL_TRUE, offset, data, &[])
                    .map_err(ClError::from)?;
                Ok(())
            }
            #[cfg(feature = "cuda")]
            (CommandQueueBackend::Cuda(stream), BufferBackend::Cuda(buf)) => {
                if offset != 0 || data.len() > buf.len {
                    return Err(ClError(CL_INVALID_VALUE));
                }
                let mut guard = buf.data.lock().expect("cuda buffer lock poisoned");
                stream
                    .memcpy_htod(data, &mut *guard)
                    .map_err(ClError::from)?;
                stream.synchronize().map_err(ClError::from)?;
                Ok(())
            }
            _ => Err(ClError(CL_INVALID_VALUE)),
        }
    }

    pub unsafe fn enqueue_read_buffer<T: GpuData + CudaData>(
        &self,
        buffer: &Buffer<T>,
        _blocking_read: ocl::types::cl_bool,
        offset: usize,
        data: &mut [T],
        _event_wait_list: &[ocl::types::cl_event],
    ) -> Result<()> {
        match (&self.backend, &buffer.backend) {
            (CommandQueueBackend::OpenCl(q), BufferBackend::OpenCl(buf)) => {
                q.enqueue_read_buffer(buf, CL_TRUE, offset, data, &[])
                    .map_err(ClError::from)?;
                Ok(())
            }
            #[cfg(feature = "cuda")]
            (CommandQueueBackend::Cuda(stream), BufferBackend::Cuda(buf)) => {
                if offset != 0 || data.len() > buf.len {
                    return Err(ClError(CL_INVALID_VALUE));
                }
                let guard = buf.data.lock().expect("cuda buffer lock poisoned");
                stream.memcpy_dtoh(&*guard, data).map_err(ClError::from)?;
                stream.synchronize().map_err(ClError::from)?;
                Ok(())
            }
            _ => Err(ClError(CL_INVALID_VALUE)),
        }
    }

    #[cfg(feature = "cuda")]
    fn cuda_stream(&self) -> Option<&Arc<CudaStream>> {
        match &self.backend {
            CommandQueueBackend::OpenCl(_) => None,
            CommandQueueBackend::Cuda(s) => Some(s),
        }
    }

    fn opencl(&self) -> Option<&ocl::command_queue::CommandQueue> {
        match &self.backend {
            CommandQueueBackend::OpenCl(q) => Some(q),
            #[cfg(feature = "cuda")]
            CommandQueueBackend::Cuda(_) => None,
        }
    }
}

#[cfg(feature = "cuda")]
pub trait CudaData: DeviceRepr {}
#[cfg(feature = "cuda")]
impl<T: DeviceRepr> CudaData for T {}

#[cfg(not(feature = "cuda"))]
pub trait CudaData {}
#[cfg(not(feature = "cuda"))]
impl<T> CudaData for T {}

pub struct Buffer<T: GpuData + CudaData> {
    backend: BufferBackend<T>,
    len: usize,
}

enum BufferBackend<T: GpuData + CudaData> {
    OpenCl(ocl::memory::Buffer<T>),
    #[cfg(feature = "cuda")]
    Cuda(CudaBuffer<T>),
}

#[cfg(feature = "cuda")]
struct CudaBuffer<T: GpuData + CudaData> {
    len: usize,
    data: Mutex<CudaSlice<T>>,
}

impl<T: GpuData + CudaData> Buffer<T> {
    pub unsafe fn create(
        context: &Context,
        flags: ocl::types::cl_mem_flags,
        size_bytes: usize,
        host_ptr: *mut c_void,
    ) -> Result<Self> {
        let elem_size = std::mem::size_of::<T>();
        if elem_size == 0 || size_bytes % elem_size != 0 {
            return Err(ClError(CL_INVALID_VALUE));
        }
        let len = size_bytes / elem_size;
        if let Some(ctx) = context.opencl() {
            let buf = ocl::memory::Buffer::create(ctx, flags, size_bytes, host_ptr)
                .map_err(ClError::from)?;
            return Ok(Self {
                backend: BufferBackend::OpenCl(buf),
                len,
            });
        }
        #[cfg(feature = "cuda")]
        {
            if let Some(stream) = context.cuda_stream() {
                let slice = unsafe { stream.alloc::<T>(len) }.map_err(ClError::from)?;
                return Ok(Self {
                    backend: BufferBackend::Cuda(CudaBuffer {
                        len,
                        data: Mutex::new(slice),
                    }),
                    len,
                });
            }
        }
        Err(ClError(CL_INVALID_VALUE))
    }

    pub fn len(&self) -> usize {
        self.len
    }

    fn opencl(&self) -> Option<&ocl::memory::Buffer<T>> {
        match &self.backend {
            BufferBackend::OpenCl(b) => Some(b),
            #[cfg(feature = "cuda")]
            BufferBackend::Cuda(_) => None,
        }
    }

    fn opencl_mut(&mut self) -> Option<&mut ocl::memory::Buffer<T>> {
        match &mut self.backend {
            BufferBackend::OpenCl(b) => Some(b),
            #[cfg(feature = "cuda")]
            BufferBackend::Cuda(_) => None,
        }
    }

    #[cfg(feature = "cuda")]
    fn cuda_lock(&self) -> Option<std::sync::MutexGuard<'_, CudaSlice<T>>> {
        match &self.backend {
            BufferBackend::OpenCl(_) => None,
            BufferBackend::Cuda(b) => Some(b.data.lock().expect("cuda buffer lock poisoned")),
        }
    }
}

pub struct Program {
    backend: ProgramBackend,
}

enum ProgramBackend {
    OpenCl(ocl::program::Program),
    #[cfg(feature = "cuda")]
    Cuda(Arc<CudaModule>),
}

impl Program {
    pub fn create_and_build_from_source(
        context: &Context,
        source: &str,
        options: &str,
    ) -> Result<Self> {
        if let Some(ctx) = context.opencl() {
            let p = ocl::program::Program::create_and_build_from_source(ctx, source, options)
                .map_err(ClError::from)?;
            return Ok(Self {
                backend: ProgramBackend::OpenCl(p),
            });
        }
        #[cfg(feature = "cuda")]
        {
            if let Some(ctx) = context.cuda_ctx() {
                let ptx = compile_ptx(source).map_err(ClError::from)?;
                let module = ctx.load_module(ptx).map_err(ClError::from)?;
                return Ok(Self {
                    backend: ProgramBackend::Cuda(module),
                });
            }
        }
        Err(ClError(CL_INVALID_VALUE))
    }
}

pub struct Kernel {
    backend: KernelBackend,
    name: String,
}

enum KernelBackend {
    OpenCl(ocl::kernel::Kernel),
    #[cfg(feature = "cuda")]
    Cuda(CudaFunction),
}

impl Kernel {
    pub fn create(program: &Program, name: &str) -> Result<Self> {
        match &program.backend {
            ProgramBackend::OpenCl(p) => {
                let k = ocl::kernel::Kernel::create(p, name).map_err(ClError::from)?;
                Ok(Self {
                    backend: KernelBackend::OpenCl(k),
                    name: name.to_string(),
                })
            }
            #[cfg(feature = "cuda")]
            ProgramBackend::Cuda(module) => {
                let k = module.load_function(name).map_err(ClError::from)?;
                Ok(Self {
                    backend: KernelBackend::Cuda(k),
                    name: name.to_string(),
                })
            }
        }
    }
}

enum KernelArg<'a> {
    BufI8(&'a Buffer<i8>),
    BufI32(&'a Buffer<i32>),
    BufF32(&'a Buffer<f32>),
    BufF64(&'a Buffer<f64>),
    BufF32x4(&'a Buffer<[f32; 4]>),
    I32(i32),
    F32(f32),
    F64(f64),
}

pub trait IntoKernelArg<'a> {
    fn into_kernel_arg(self) -> KernelArg<'a>;
}

macro_rules! impl_buf_arg {
    ($t:ty, $v:ident) => {
        impl<'a> IntoKernelArg<'a> for &'a Buffer<$t> {
            fn into_kernel_arg(self) -> KernelArg<'a> {
                KernelArg::$v(self)
            }
        }
        impl<'a> IntoKernelArg<'a> for &'a mut Buffer<$t> {
            fn into_kernel_arg(self) -> KernelArg<'a> {
                KernelArg::$v(self)
            }
        }
    };
}

impl_buf_arg!(i8, BufI8);
impl_buf_arg!(i32, BufI32);
impl_buf_arg!(f32, BufF32);
impl_buf_arg!(f64, BufF64);
impl_buf_arg!([f32; 4], BufF32x4);

impl<'a> IntoKernelArg<'a> for &'a i32 {
    fn into_kernel_arg(self) -> KernelArg<'a> {
        KernelArg::I32(*self)
    }
}

impl<'a> IntoKernelArg<'a> for &'a f32 {
    fn into_kernel_arg(self) -> KernelArg<'a> {
        KernelArg::F32(*self)
    }
}

impl<'a> IntoKernelArg<'a> for &'a f64 {
    fn into_kernel_arg(self) -> KernelArg<'a> {
        KernelArg::F64(*self)
    }
}

pub struct ExecuteKernel<'a> {
    kernel: &'a Kernel,
    args: Vec<KernelArg<'a>>,
    global_sizes: Vec<usize>,
}

impl<'a> ExecuteKernel<'a> {
    pub fn new(kernel: &'a Kernel) -> Self {
        Self {
            kernel,
            args: Vec::new(),
            global_sizes: Vec::new(),
        }
    }

    pub unsafe fn set_arg<A: IntoKernelArg<'a>>(mut self, arg: A) -> Self {
        self.args.push(arg.into_kernel_arg());
        self
    }

    pub fn set_global_work_size(mut self, size: usize) -> Self {
        self.global_sizes.clear();
        self.global_sizes.push(size);
        self
    }

    pub fn set_global_work_sizes(mut self, sizes: &[usize]) -> Self {
        self.global_sizes.clear();
        self.global_sizes.extend_from_slice(sizes);
        self
    }

    pub unsafe fn enqueue_nd_range(self, queue: &CommandQueue) -> Result<()> {
        match (&self.kernel.backend, queue.opencl()) {
            (KernelBackend::OpenCl(kernel), Some(ocl_queue)) => {
                for (idx, arg) in self.args.iter().enumerate() {
                    let i = idx as u32;
                    match arg {
                        KernelArg::BufI8(b) => {
                            let bb = b.opencl().ok_or(ClError(CL_INVALID_VALUE))?;
                            kernel.set_arg(i, bb).map_err(ClError::from)?;
                        }
                        KernelArg::BufI32(b) => {
                            let bb = b.opencl().ok_or(ClError(CL_INVALID_VALUE))?;
                            kernel.set_arg(i, bb).map_err(ClError::from)?;
                        }
                        KernelArg::BufF32(b) => {
                            let bb = b.opencl().ok_or(ClError(CL_INVALID_VALUE))?;
                            kernel.set_arg(i, bb).map_err(ClError::from)?;
                        }
                        KernelArg::BufF64(b) => {
                            let bb = b.opencl().ok_or(ClError(CL_INVALID_VALUE))?;
                            kernel.set_arg(i, bb).map_err(ClError::from)?;
                        }
                        KernelArg::BufF32x4(b) => {
                            let bb = b.opencl().ok_or(ClError(CL_INVALID_VALUE))?;
                            kernel.set_arg(i, bb).map_err(ClError::from)?;
                        }
                        KernelArg::I32(v) => kernel.set_arg(i, v).map_err(ClError::from)?,
                        KernelArg::F32(v) => kernel.set_arg(i, v).map_err(ClError::from)?,
                        KernelArg::F64(v) => kernel.set_arg(i, v).map_err(ClError::from)?,
                    }
                }

                let mut gws = [1usize; 3];
                let dim = self.global_sizes.len().max(1).min(3);
                for (i, s) in self.global_sizes.iter().copied().take(dim).enumerate() {
                    gws[i] = s.max(1);
                }
                ocl_queue
                    .enqueue_nd_range_kernel(
                        kernel.get(),
                        dim as u32,
                        ptr::null(),
                        gws.as_ptr(),
                        ptr::null(),
                        &[],
                    )
                    .map_err(ClError::from)?;
                return Ok(());
            }
            _ => {}
        }

        #[cfg(feature = "cuda")]
        {
            self.enqueue_cuda(queue)
        }
        #[cfg(not(feature = "cuda"))]
        {
            Err(ClError(CL_INVALID_VALUE))
        }
    }

    #[cfg(feature = "cuda")]
    unsafe fn enqueue_cuda(self, queue: &CommandQueue) -> Result<()> {
        use KernelArg::*;

        let stream = queue.cuda_stream().ok_or(ClError(CL_INVALID_VALUE))?;
        let kernel = match &self.kernel.backend {
            KernelBackend::Cuda(k) => k,
            _ => return Err(ClError(CL_INVALID_VALUE)),
        };

        let g0 = self.global_sizes.first().copied().unwrap_or(1).max(1);
        let cfg_1d = LaunchConfig::for_num_elems(g0.min(u32::MAX as usize) as u32);

        macro_rules! get {
            ($idx:expr, I32) => {
                match self.args.get($idx) {
                    Some(I32(v)) => *v,
                    _ => return Err(ClError(CL_INVALID_VALUE)),
                }
            };
            ($idx:expr, F32) => {
                match self.args.get($idx) {
                    Some(F32(v)) => *v,
                    _ => return Err(ClError(CL_INVALID_VALUE)),
                }
            };
            ($idx:expr, F64) => {
                match self.args.get($idx) {
                    Some(F64(v)) => *v,
                    _ => return Err(ClError(CL_INVALID_VALUE)),
                }
            };
            ($idx:expr, BufI8) => {
                match self.args.get($idx) {
                    Some(BufI8(b)) => *b,
                    _ => return Err(ClError(CL_INVALID_VALUE)),
                }
            };
            ($idx:expr, BufI32) => {
                match self.args.get($idx) {
                    Some(BufI32(b)) => *b,
                    _ => return Err(ClError(CL_INVALID_VALUE)),
                }
            };
            ($idx:expr, BufF32) => {
                match self.args.get($idx) {
                    Some(BufF32(b)) => *b,
                    _ => return Err(ClError(CL_INVALID_VALUE)),
                }
            };
            ($idx:expr, BufF64) => {
                match self.args.get($idx) {
                    Some(BufF64(b)) => *b,
                    _ => return Err(ClError(CL_INVALID_VALUE)),
                }
            };
            ($idx:expr, BufF32x4) => {
                match self.args.get($idx) {
                    Some(BufF32x4(b)) => *b,
                    _ => return Err(ClError(CL_INVALID_VALUE)),
                }
            };
        }

        match self.kernel.name.as_str() {
            "lif_step" => {
                let v_buf = get!(0, BufF64);
                let n_neurons = v_buf.len().min(i32::MAX as usize) as i32;
                let mut v = v_buf.cuda_lock().ok_or(ClError(CL_INVALID_VALUE))?;
                let mut refr = get!(1, BufI32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut i_total = get!(2, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let decay_m = get!(3, F64);
                let v_th = get!(4, F64);
                let v_reset = get!(5, F64);
                let refractory_steps = get!(6, I32);
                let mut spk = get!(7, BufI8)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                stream
                    .launch_builder(kernel)
                    .arg(&mut *v)
                    .arg(&mut *refr)
                    .arg(&mut *i_total)
                    .arg(&decay_m)
                    .arg(&v_th)
                    .arg(&v_reset)
                    .arg(&refractory_steps)
                    .arg(&mut *spk)
                    .arg(&n_neurons)
                    .launch(cfg_1d)
                    .map_err(ClError::from)?;
            }
            "izh_step" => {
                let v_buf = get!(0, BufF64);
                let n_neurons = v_buf.len().min(i32::MAX as usize) as i32;
                let mut v = v_buf.cuda_lock().ok_or(ClError(CL_INVALID_VALUE))?;
                let mut u = get!(1, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut i_total = get!(2, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let dt = get!(3, F64);
                let a = get!(4, F64);
                let b = get!(5, F64);
                let c = get!(6, F64);
                let d = get!(7, F64);
                let v_th = get!(8, F64);
                let mut spk = get!(9, BufI8)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                stream
                    .launch_builder(kernel)
                    .arg(&mut *v)
                    .arg(&mut *u)
                    .arg(&mut *i_total)
                    .arg(&dt)
                    .arg(&a)
                    .arg(&b)
                    .arg(&c)
                    .arg(&d)
                    .arg(&v_th)
                    .arg(&mut *spk)
                    .arg(&n_neurons)
                    .launch(cfg_1d)
                    .map_err(ClError::from)?;
            }
            "syn_acc_dense" => {
                let mut i_acc = get!(0, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut pre = get!(1, BufI8)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut w = get!(2, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let n_pre = get!(3, I32);
                let n_post = get!(4, I32);
                stream
                    .launch_builder(kernel)
                    .arg(&mut *i_acc)
                    .arg(&mut *pre)
                    .arg(&mut *w)
                    .arg(&n_pre)
                    .arg(&n_post)
                    .launch(cfg_1d)
                    .map_err(ClError::from)?;
            }
            "syn_acc_dense_stp" => {
                let mut i_acc = get!(0, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut rel = get!(1, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut w = get!(2, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let n_pre = get!(3, I32);
                let n_post = get!(4, I32);
                stream
                    .launch_builder(kernel)
                    .arg(&mut *i_acc)
                    .arg(&mut *rel)
                    .arg(&mut *w)
                    .arg(&n_pre)
                    .arg(&n_post)
                    .launch(cfg_1d)
                    .map_err(ClError::from)?;
            }
            "syn_acc_sparse" => {
                let mut i_acc = get!(0, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut pre = get!(1, BufI8)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut row_ptr = get!(2, BufI32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut col = get!(3, BufI32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut w = get!(4, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let n_post = get!(5, I32);
                let accumulate = get!(6, I32);
                stream
                    .launch_builder(kernel)
                    .arg(&mut *i_acc)
                    .arg(&mut *pre)
                    .arg(&mut *row_ptr)
                    .arg(&mut *col)
                    .arg(&mut *w)
                    .arg(&n_post)
                    .arg(&accumulate)
                    .launch(cfg_1d)
                    .map_err(ClError::from)?;
            }
            "syn_acc_sparse_stp" => {
                let mut i_acc = get!(0, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut pre = get!(1, BufI8)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut rel = get!(2, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut row_ptr = get!(3, BufI32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut col = get!(4, BufI32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut w = get!(5, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let n_post = get!(6, I32);
                let accumulate = get!(7, I32);
                stream
                    .launch_builder(kernel)
                    .arg(&mut *i_acc)
                    .arg(&mut *pre)
                    .arg(&mut *rel)
                    .arg(&mut *row_ptr)
                    .arg(&mut *col)
                    .arg(&mut *w)
                    .arg(&n_post)
                    .arg(&accumulate)
                    .launch(cfg_1d)
                    .map_err(ClError::from)?;
            }
            "syn_acc_sparse_delay" => {
                let mut i_acc = get!(0, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut hist = get!(1, BufI8)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut row_ptr = get!(2, BufI32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut col = get!(3, BufI32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut delays = get!(4, BufI32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut w = get!(5, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let n_post = get!(6, I32);
                let hist_len = get!(7, I32);
                let neurons_per_frame = get!(8, I32);
                let accumulate = get!(9, I32);
                stream
                    .launch_builder(kernel)
                    .arg(&mut *i_acc)
                    .arg(&mut *hist)
                    .arg(&mut *row_ptr)
                    .arg(&mut *col)
                    .arg(&mut *delays)
                    .arg(&mut *w)
                    .arg(&n_post)
                    .arg(&hist_len)
                    .arg(&neurons_per_frame)
                    .arg(&accumulate)
                    .launch(cfg_1d)
                    .map_err(ClError::from)?;
            }
            "syn_acc_sparse_delay_stp" => {
                let mut i_acc = get!(0, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut hist = get!(1, BufI8)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut rel = get!(2, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut row_ptr = get!(3, BufI32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut col = get!(4, BufI32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut delays = get!(5, BufI32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut w = get!(6, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let n_post = get!(7, I32);
                let hist_len = get!(8, I32);
                let neurons_per_frame = get!(9, I32);
                let accumulate = get!(10, I32);
                stream
                    .launch_builder(kernel)
                    .arg(&mut *i_acc)
                    .arg(&mut *hist)
                    .arg(&mut *rel)
                    .arg(&mut *row_ptr)
                    .arg(&mut *col)
                    .arg(&mut *delays)
                    .arg(&mut *w)
                    .arg(&n_post)
                    .arg(&hist_len)
                    .arg(&neurons_per_frame)
                    .arg(&accumulate)
                    .launch(cfg_1d)
                    .map_err(ClError::from)?;
            }
            "syn_filter" => {
                let i_acc_buf = get!(0, BufF64);
                let n_post = i_acc_buf.len().min(i32::MAX as usize) as i32;
                let mut i_acc = i_acc_buf.cuda_lock().ok_or(ClError(CL_INVALID_VALUE))?;
                let mut ampa = get!(1, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut nmda = get!(2, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut gaba = get!(3, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let decay_ampa = get!(4, F64);
                let decay_nmda = get!(5, F64);
                let decay_gaba = get!(6, F64);
                let nmda_ratio = get!(7, F64);
                let syn_gain = get!(8, F64);
                stream
                    .launch_builder(kernel)
                    .arg(&mut *i_acc)
                    .arg(&mut *ampa)
                    .arg(&mut *nmda)
                    .arg(&mut *gaba)
                    .arg(&decay_ampa)
                    .arg(&decay_nmda)
                    .arg(&decay_gaba)
                    .arg(&nmda_ratio)
                    .arg(&syn_gain)
                    .arg(&n_post)
                    .launch(cfg_1d)
                    .map_err(ClError::from)?;
            }
            "stp_update" => {
                let mut u = get!(0, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut x = get!(1, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut pre = get!(2, BufI8)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let rel_buf = get!(3, BufF64);
                let n_pre = rel_buf.len().min(i32::MAX as usize) as i32;
                let mut rel = rel_buf.cuda_lock().ok_or(ClError(CL_INVALID_VALUE))?;
                let stp_u = get!(4, F64);
                let decay_rec = get!(5, F64);
                let decay_facil = get!(6, F64);
                stream
                    .launch_builder(kernel)
                    .arg(&mut *u)
                    .arg(&mut *x)
                    .arg(&mut *pre)
                    .arg(&mut *rel)
                    .arg(&stp_u)
                    .arg(&decay_rec)
                    .arg(&decay_facil)
                    .arg(&n_pre)
                    .launch(cfg_1d)
                    .map_err(ClError::from)?;
            }
            "plasticity_update" => {
                let mut w = get!(0, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut pre = get!(1, BufI8)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut post = get!(2, BufI8)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut x_pre = get!(3, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut x_post = get!(4, BufF64)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let eta = get!(5, F64);
                let w_min = get!(6, F64);
                let w_max = get!(7, F64);
                let n_pre = get!(8, I32);
                let n_post = get!(9, I32);
                let rule = get!(10, I32);
                let gx = self
                    .global_sizes
                    .first()
                    .copied()
                    .unwrap_or(n_post as usize)
                    .max(1);
                let gy = self
                    .global_sizes
                    .get(1)
                    .copied()
                    .unwrap_or(n_pre as usize)
                    .max(1);
                let gx_u32 = gx.min(u32::MAX as usize) as u32;
                let gy_u32 = gy.min(u32::MAX as usize) as u32;
                let cfg = LaunchConfig {
                    grid_dim: (
                        gx_u32.saturating_add(15) / 16,
                        gy_u32.saturating_add(15) / 16,
                        1,
                    ),
                    block_dim: (16, 16, 1),
                    shared_mem_bytes: 0,
                };
                stream
                    .launch_builder(kernel)
                    .arg(&mut *w)
                    .arg(&mut *pre)
                    .arg(&mut *post)
                    .arg(&mut *x_pre)
                    .arg(&mut *x_post)
                    .arg(&eta)
                    .arg(&w_min)
                    .arg(&w_max)
                    .arg(&n_pre)
                    .arg(&n_post)
                    .arg(&rule)
                    .launch(cfg)
                    .map_err(ClError::from)?;
            }
            "morpho_energy" => {
                let mut points = get!(0, BufF32x4)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut syn_sites = get!(1, BufF32x4)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let mut syn_stim = get!(2, BufF32)
                    .cuda_lock()
                    .ok_or(ClError(CL_INVALID_VALUE))?;
                let energies_buf = get!(3, BufF32);
                let n_points = energies_buf.len().min(i32::MAX as usize) as i32;
                let mut energies = energies_buf.cuda_lock().ok_or(ClError(CL_INVALID_VALUE))?;
                let n_syn = get!(4, I32);
                let radius_sq = get!(5, F32);
                let kernel_k = get!(6, F32);
                stream
                    .launch_builder(kernel)
                    .arg(&mut *points)
                    .arg(&mut *syn_sites)
                    .arg(&mut *syn_stim)
                    .arg(&mut *energies)
                    .arg(&n_syn)
                    .arg(&radius_sq)
                    .arg(&kernel_k)
                    .arg(&n_points)
                    .launch(cfg_1d)
                    .map_err(ClError::from)?;
            }
            _ => return Err(ClError(CL_INVALID_VALUE)),
        }

        stream.synchronize().map_err(ClError::from)?;
        Ok(())
    }
}
