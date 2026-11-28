// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
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
/// One line of an overlay figure: legend name, colour and (x, y) points.
pub struct FigureLine {
    pub name: String,
    pub rgb: [f32; 3],
    pub points: Vec<[f32; 2]>,
}
/// A 2D figure drawn over the scene (MuJoCo `mjr_figure`). `rect` is
/// `[left, bottom, width, height]` as fractions of the framebuffer.
pub struct FigureSpec {
    pub title: String,
    pub rect: [f32; 4],
    pub x_range: [f32; 2],
    pub y_range: [f32; 2],
    pub lines: Vec<FigureLine>,
}
/// Text drawn over the scene (MuJoCo `mjr_overlay`): grid position (0 top
/// left, 1 top right, 2 bottom left, 3 bottom right), left and right columns.
pub struct OverlayText {
    pub gridpos: i32,
    pub left: String,
    pub right: String,
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
        Self::open(physics, title, habitat_extent, close_view, (1100, 760))
    }
    /// A window of an exact size for offscreen-style frame capture (trace
    /// replay videos on a virtual display).
    pub fn new_sized(
        physics: &Physics,
        title: &str,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        if !(16..=4096).contains(&width) || !(16..=4096).contains(&height) {
            return Err("replay frame size".into());
        }
        Self::open(physics, title, None, true, (width, height))
    }
    /// Place the camera (look-at point, distance, azimuth and elevation in degrees).
    pub fn set_camera(&mut self, lookat: [f64; 3], distance: f64, azimuth: f64, elevation: f64) {
        self.camera.lookat = lookat;
        self.camera.distance = distance;
        self.camera.azimuth = azimuth;
        self.camera.elevation = elevation;
    }
    /// Render one frame with text and figure overlays and return its RGB
    /// pixels, top row first, with the framebuffer size.
    pub fn capture(
        &mut self,
        physics: &mut Physics,
        texts: &[OverlayText],
        figures: &[FigureSpec],
    ) -> Result<(usize, usize, Vec<u8>), String> {
        let texts = texts
            .iter()
            .map(|t| {
                Ok((
                    t.gridpos,
                    CString::new(t.left.as_str()).map_err(|_| "overlay text")?,
                    CString::new(t.right.as_str()).map_err(|_| "overlay text")?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        // SAFETY: live matching model/data and rendering allocations; the GLFW
        // context is made current before any OpenGL call. The figure is a
        // heap allocation initialised by mjv_defaultFigure; line counts are
        // bounded by mjMAXLINE / mjMAXLINEPNT before writing.
        unsafe {
            (self.glfw.current)(self.window);
            let mut width = 0;
            let mut height = 0;
            (self.glfw.size)(self.window, &mut width, &mut height);
            if width <= 0 || height <= 0 || width > 8192 || height > 8192 {
                return Err("replay framebuffer size".into());
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
            for spec in figures {
                let mut figure: Box<ffi::mjvFigure> = Box::new(std::mem::zeroed());
                ffi::mjv_defaultFigure(&mut *figure);
                figure.flg_legend = 1;
                figure.flg_ticklabel = [1, 1];
                figure.flg_extend = 0;
                figure.linewidth = 2.0;
                // MuJoCo rounds the y-range outward to the order of magnitude
                // of its span (0.8-2.2 -> 0-3, -20-120 -> -100-200): callers
                // choose ranges whose ends are multiples of that magnitude.
                figure.gridsize = [5, 7];
                // Tick labels: integers, or one decimal on a small y-range (the
                // default %g turns 200 into 2e+02; %.0f repeats on 0-2.5).
                let fmt = |small: bool| {
                    let f = if small { b"%.1f\0" } else { b"%.0f\0" };
                    let mut out = [0 as c_char; 5];
                    for (o, b) in out.iter_mut().zip(f) {
                        *o = *b as c_char;
                    }
                    out
                };
                figure.xformat[..5].copy_from_slice(&fmt(false));
                figure.yformat[..5]
                    .copy_from_slice(&fmt((spec.y_range[1] - spec.y_range[0]).abs() <= 5.));
                figure.figurergba = [0.08, 0.08, 0.08, 0.75];
                figure.panergba = [0.12, 0.12, 0.12, 0.85];
                figure.legendrgba = [0.0, 0.0, 0.0, 0.25];
                figure.range = [spec.x_range, spec.y_range];
                let copy = |dst: &mut [c_char], src: &str| {
                    let room = dst.len() - 1;
                    for (d, b) in dst
                        .iter_mut()
                        .zip(src.bytes().take(room).chain(std::iter::once(0)))
                    {
                        *d = b as c_char;
                    }
                };
                copy(&mut figure.title, &spec.title);
                for (k, line) in spec.lines.iter().take(figure.linepnt.len()).enumerate() {
                    copy(&mut figure.linename[k], &line.name);
                    figure.linergb[k] = line.rgb;
                    let max_points = figure.linedata[k].len() / 2;
                    let points = &line.points[line.points.len().saturating_sub(max_points)..];
                    for (j, p) in points.iter().enumerate() {
                        figure.linedata[k][2 * j] = p[0];
                        figure.linedata[k][2 * j + 1] = p[1];
                    }
                    figure.linepnt[k] = points.len() as c_int;
                }
                let rect = ffi::mjrRect {
                    left: (spec.rect[0] * width as f32) as c_int,
                    bottom: (spec.rect[1] * height as f32) as c_int,
                    width: (spec.rect[2] * width as f32) as c_int,
                    height: (spec.rect[3] * height as f32) as c_int,
                };
                ffi::mjr_figure(rect, &mut *figure, &*self.context);
            }
            for (gridpos, left, right) in &texts {
                ffi::mjr_overlay(
                    ffi::mjtFont__mjFONT_SHADOW as c_int,
                    *gridpos,
                    viewport,
                    left.as_ptr(),
                    right.as_ptr(),
                    &*self.context,
                );
            }
            let mut rgb = vec![0u8; width as usize * height as usize * 3];
            ffi::mjr_readPixels(
                rgb.as_mut_ptr(),
                std::ptr::null_mut(),
                viewport,
                &*self.context,
            );
            let row = width as usize * 3;
            let mut flipped = Vec::with_capacity(rgb.len());
            for line in rgb.chunks_exact(row).rev() {
                flipped.extend_from_slice(line);
            }
            (self.glfw.swap)(self.window);
            Ok((width as usize, height as usize, flipped))
        }
    }
    fn open(
        physics: &Physics,
        title: &str,
        habitat_extent: Option<f64>,
        close_view: bool,
        size: (i32, i32),
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
                size.0,
                size.1,
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
