//! Main-thread GLFW/OpenGL window rendering the same native physics owner.
//! GLFW is loaded at runtime; headless/service builds require no window stack.
use super::{Physics, ffi};
use std::{
    cell::Cell,
    ffi::{CString, c_char, c_int, c_void},
    path::Path,
};
type Window = *mut c_void;
thread_local! { static USERS:Cell<usize> = const { Cell::new(0) }; }
struct Glfw {
    _library: libloading::Library,
    init: unsafe extern "C" fn() -> c_int,
    terminate: unsafe extern "C" fn(),
    create: unsafe extern "C" fn(c_int, c_int, *const c_char, Window, Window) -> Window,
    destroy: unsafe extern "C" fn(Window),
    current: unsafe extern "C" fn(Window),
    interval: unsafe extern "C" fn(c_int),
    closed: unsafe extern "C" fn(Window) -> c_int,
    poll: unsafe extern "C" fn(),
    swap: unsafe extern "C" fn(Window),
    size: unsafe extern "C" fn(Window, *mut c_int, *mut c_int),
    key: unsafe extern "C" fn(Window, c_int) -> c_int,
    button: unsafe extern "C" fn(Window, c_int) -> c_int,
    cursor: unsafe extern "C" fn(Window, *mut f64, *mut f64),
    title: unsafe extern "C" fn(Window, *const c_char),
}
impl Glfw {
    fn load() -> Result<Self, String> {
        // SAFETY: GLFW's versioned 3.x C ABI; symbols are kept alive by library.
        unsafe {
            let library = libloading::Library::new("libglfw.so.3").map_err(|e| {
                format!("GLFW 3 runtime unavailable: {e}; use --headless --steps N for diagnostics")
            })?;
            macro_rules! symbol {
                ($name:literal) => {
                    *library
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|e| e.to_string())?
                };
            }
            Ok(Self {
                init: symbol!("glfwInit"),
                terminate: symbol!("glfwTerminate"),
                create: symbol!("glfwCreateWindow"),
                destroy: symbol!("glfwDestroyWindow"),
                current: symbol!("glfwMakeContextCurrent"),
                interval: symbol!("glfwSwapInterval"),
                closed: symbol!("glfwWindowShouldClose"),
                poll: symbol!("glfwPollEvents"),
                swap: symbol!("glfwSwapBuffers"),
                size: symbol!("glfwGetFramebufferSize"),
                key: symbol!("glfwGetKey"),
                button: symbol!("glfwGetMouseButton"),
                cursor: symbol!("glfwGetCursorPos"),
                title: symbol!("glfwSetWindowTitle"),
                _library: library,
            })
        }
    }
}
pub struct Viewer {
    glfw: Glfw,
    window: Window,
    scene: Box<ffi::mjvScene>,
    context: Box<ffi::mjrContext>,
    camera: ffi::mjvCamera,
    options: ffi::mjvOption,
    cursor: [f64; 2],
    paused: bool,
    previous_space: bool,
    previous_reset: bool,
    previous_focus: bool,
    close_view: bool,
    distances: [f64; 2],
}
impl Viewer {
    pub fn new(
        physics: &Physics,
        title: &str,
        habitat_extent: Option<f64>,
        close_view: bool,
    ) -> Result<Self, String> {
        let title = CString::new(title).map_err(|_| "window title")?;
        let glfw = Glfw::load()?;
        // SAFETY: called only on the main thread. All native storage is
        // initialized by its documented default function before use.
        unsafe {
            if USERS.with(|n| n.get() == 0) && (glfw.init)() == 0 {
                return Err(
                    "GLFW cannot open a display; use --headless --steps N for diagnostics".into(),
                );
            }
            let window = (glfw.create)(
                1100,
                760,
                title.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if window.is_null() {
                if USERS.with(|n| n.get() == 0) {
                    (glfw.terminate)();
                }
                return Err("GLFW OpenGL window creation failed".into());
            }
            USERS.with(|n| n.set(n.get() + 1));
            (glfw.current)(window);
            (glfw.interval)(0);
            let mut scene: Box<ffi::mjvScene> = Box::new(std::mem::zeroed());
            let mut context: Box<ffi::mjrContext> = Box::new(std::mem::zeroed());
            let mut camera = std::mem::zeroed();
            let mut options = std::mem::zeroed();
            ffi::mjv_defaultScene(&mut *scene);
            ffi::mjv_makeScene(physics.model.as_ptr(), &mut *scene, 10000);
            ffi::mjr_defaultContext(&mut *context);
            ffi::mjr_makeContext(physics.model.as_ptr(), &mut *context, 150);
            ffi::mjv_defaultCamera(&mut camera);
            ffi::mjv_defaultOption(&mut options);
            // Frame the articulated body, excluding the floor and distant
            // inactive voxel mirror slots from the camera bounds.
            let positions: Vec<_> = (1..physics.m().nbody as usize)
                .filter(|i| *physics.m().body_mocapid.add(*i) < 0)
                .map(|i| std::array::from_fn::<_, 3, _>(|j| *physics.d().xpos.add(3 * i + j)))
                .collect();
            let center: [f64; 3] = std::array::from_fn(|j| {
                positions.iter().map(|p| p[j]).sum::<f64>() / positions.len().max(1) as f64
            });
            camera.lookat = center;
            camera.distance = positions
                .iter()
                .map(|p| {
                    p.iter()
                        .zip(center)
                        .map(|(a, b)| (a - b).powi(2))
                        .sum::<f64>()
                        .sqrt()
                })
                .fold(0.1, f64::max)
                * 3.5;
            camera.azimuth = 100.;
            camera.elevation = -48.;
            let distances = [
                camera.distance,
                habitat_extent
                    .filter(|x| x.is_finite() && *x > 0.)
                    .map_or(camera.distance, |x| (x * 2.6).max(camera.distance)),
            ];
            camera.distance = distances[usize::from(!close_view)];
            Ok(Self {
                glfw,
                window,
                scene,
                context,
                camera,
                options,
                cursor: [0.; 2],
                paused: false,
                previous_space: false,
                previous_reset: false,
                previous_focus: false,
                close_view,
                distances,
            })
        }
    }
    pub fn poll(&mut self, physics: &Physics) -> (bool, bool, bool) {
        // SAFETY: live GLFW window on its main owner thread; output pointers
        // refer to initialized stack scalars. Key constants are GLFW 3 values.
        unsafe {
            (self.glfw.poll)();
            let space = (self.glfw.key)(self.window, 32) == 1;
            if space && !self.previous_space {
                self.paused = !self.paused;
            }
            self.previous_space = space;
            let reset = (self.glfw.key)(self.window, 82) == 1;
            let reset_edge = reset && !self.previous_reset;
            self.previous_reset = reset;
            let focus = (self.glfw.key)(self.window, 70) == 1;
            if focus && !self.previous_focus {
                self.close_view = !self.close_view;
                self.camera.distance = self.distances[usize::from(!self.close_view)];
            }
            self.previous_focus = focus;
            let mut x = 0.;
            let mut y = 0.;
            (self.glfw.cursor)(self.window, &mut x, &mut y);
            let dx = x - self.cursor[0];
            let dy = y - self.cursor[1];
            self.cursor = [x, y];
            if (self.glfw.button)(self.window, 0) == 1 {
                ffi::mjv_moveCamera(
                    physics.model.as_ptr(),
                    ffi::mjtMouse__mjMOUSE_ROTATE_V as i32,
                    dx / 760.,
                    dy / 760.,
                    &mut self.camera,
                );
            }
            if (self.glfw.button)(self.window, 1) == 1 {
                ffi::mjv_moveCamera(
                    physics.model.as_ptr(),
                    ffi::mjtMouse__mjMOUSE_ZOOM as i32,
                    dx / 760.,
                    dy / 760.,
                    &mut self.camera,
                );
            }
            if (self.glfw.key)(self.window, 61) == 1 {
                self.camera.distance *= 0.98;
            }
            if (self.glfw.key)(self.window, 45) == 1 {
                self.camera.distance *= 1.02;
            }
            (
                (self.glfw.closed)(self.window) != 0 || (self.glfw.key)(self.window, 256) == 1,
                self.paused,
                reset_edge,
            )
        }
    }
    pub fn render(
        &mut self,
        physics: &mut Physics,
        title: &str,
        screenshot: Option<&Path>,
    ) -> Result<(), String> {
        let title = CString::new(title).map_err(|_| "window title")?;
        // SAFETY: live matching model/data and rendering allocations; GLFW
        // context is current before any OpenGL call. No physics thread races.
        unsafe {
            (self.glfw.current)(self.window);
            (self.glfw.title)(self.window, title.as_ptr());
            let mut width = 0;
            let mut height = 0;
            (self.glfw.size)(self.window, &mut width, &mut height);
            if width <= 0 || height <= 0 {
                return Ok(());
            }
            let viewport = ffi::mjrRect {
                left: 0,
                bottom: 0,
                width,
                height,
            };
            ffi::mjv_updateScene(
                physics.model.as_ptr(),
                physics.data.as_ptr(),
                &self.options,
                std::ptr::null(),
                &mut self.camera,
                ffi::mjtCatBit__mjCAT_ALL as i32,
                &mut *self.scene,
            );
            ffi::mjr_render(viewport, &mut *self.scene, &*self.context);
            if let Some(path) = screenshot {
                if width > 8192 || height > 8192 {
                    return Err("screenshot framebuffer budget".into());
                }
                let mut rgb = vec![0u8; width as usize * height as usize * 3];
                ffi::mjr_readPixels(
                    rgb.as_mut_ptr(),
                    std::ptr::null_mut(),
                    viewport,
                    &*self.context,
                );
                let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
                for row in rgb.chunks_exact(width as usize * 3).rev() {
                    ppm.extend_from_slice(row);
                }
                std::fs::write(path, ppm).map_err(|e| e.to_string())?;
            }
            (self.glfw.swap)(self.window);
        }
        Ok(())
    }
}
impl Drop for Viewer {
    fn drop(&mut self) {
        // SAFETY: same-thread live context; free rendering resources before
        // destroying its window, then terminate only after the final window.
        unsafe {
            (self.glfw.current)(self.window);
            ffi::mjr_freeContext(&mut *self.context);
            ffi::mjv_freeScene(&mut *self.scene);
            (self.glfw.destroy)(self.window);
            if USERS.with(|n| {
                n.set(n.get() - 1);
                n.get() == 0
            }) {
                (self.glfw.terminate)();
            }
        }
    }
}
