//! Minimal audited Webots R2025a C boundary, confined to its controller thread.
use std::{
    ffi::{CString, c_char, c_void},
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
};
type Tag = u16;
/// SI world position, body-to-world rotation, linear/angular velocity and COM.
pub type BodyState = ([f64; 3], [[f64; 3]; 3], [f64; 6], [f64; 3]);
unsafe extern "C" {
    fn wb_robot_init() -> i32;
    fn wb_robot_cleanup();
    fn wb_robot_step(ms: i32) -> i32;
    fn wb_robot_get_device(name: *const c_char) -> Tag;
    fn wb_position_sensor_enable(tag: Tag, ms: i32);
    fn wb_position_sensor_get_value(tag: Tag) -> f64;
    fn wb_motor_set_torque(tag: Tag, value: f64);
    fn wb_motor_set_force(tag: Tag, value: f64);
    fn wb_gyro_enable(tag: Tag, ms: i32);
    fn wb_gyro_get_values(tag: Tag) -> *const f64;
    fn wb_accelerometer_enable(tag: Tag, ms: i32);
    fn wb_accelerometer_get_values(tag: Tag) -> *const f64;
    fn wb_distance_sensor_enable(tag: Tag, ms: i32);
    fn wb_distance_sensor_get_value(tag: Tag) -> f64;
    fn wb_camera_enable(tag: Tag, ms: i32);
    fn wb_camera_get_image(tag: Tag) -> *const u8;
    fn wb_camera_get_width(tag: Tag) -> i32;
    fn wb_camera_get_height(tag: Tag) -> i32;
    fn wb_supervisor_node_get_from_def(name: *const c_char) -> *mut c_void;
    fn wb_supervisor_node_get_position(node: *mut c_void) -> *const f64;
    fn wb_supervisor_node_get_orientation(node: *mut c_void) -> *const f64;
    fn wb_supervisor_node_get_velocity(node: *mut c_void) -> *const f64;
    fn wb_supervisor_node_get_center_of_mass(node: *mut c_void) -> *const f64;
    fn wb_supervisor_node_get_contact_points(
        node: *mut c_void,
        descendants: c_char,
        count: *mut i32,
    ) -> *const c_void;
    fn wb_supervisor_node_enable_contact_points_tracking(
        node: *mut c_void,
        ms: i32,
        descendants: c_char,
    );
    fn wb_supervisor_node_add_force_with_offset(
        node: *mut c_void,
        force: *const f64,
        offset: *const f64,
        relative: c_char,
    );
    fn wb_supervisor_node_save_state(node: *mut c_void, name: *const c_char);
    fn wb_supervisor_node_load_state(node: *mut c_void, name: *const c_char);
    fn wb_supervisor_simulation_quit(status: i32);
}
#[derive(Clone, Copy)]
pub struct Node(NonNull<c_void>, PhantomData<Rc<()>>);
pub struct Controller {
    motors: Vec<(Tag, bool)>,
    _thread: PhantomData<Rc<()>>,
}
impl Controller {
    pub fn init() -> Self {
        // SAFETY: Called once by the Webots controller process on its owner thread.
        unsafe {
            wb_robot_init();
        }
        Self {
            motors: Vec::new(),
            _thread: PhantomData,
        }
    }
    pub fn step(&mut self) -> bool {
        // SAFETY: Initialized, live thread-affine controller session; 2ms equals
        // the generated world's basicTimeStep and never advances neural time.
        unsafe { wb_robot_step(2) != -1 }
    }
    pub fn device(&self, name: &str) -> Result<Tag, String> {
        let name = CString::new(name).map_err(|_| "invalid device name")?;
        // SAFETY: NUL-terminated name alive during call, native tag copied.
        let tag = unsafe { wb_robot_get_device(name.as_ptr()) };
        if tag == 0 {
            Err("missing named Webots device".into())
        } else {
            Ok(tag)
        }
    }
    pub fn joint(
        &mut self,
        sensor: &str,
        motor: Option<(&str, bool)>,
    ) -> Result<(Tag, Option<Tag>), String> {
        let s = self.device(sensor)?;
        // SAFETY: Named generated device is a PositionSensor, period is valid.
        unsafe {
            wb_position_sensor_enable(s, 2);
        }
        let m = if let Some((name, linear)) = motor {
            let tag = self.device(name)?;
            self.motors.push((tag, linear));
            self.effort(tag, linear, 0.)?;
            Some(tag)
        } else {
            None
        };
        Ok((s, m))
    }
    pub fn position(&self, tag: Tag) -> Result<f64, String> {
        // SAFETY: Caller only passes initialized position-sensor tags.
        let value = unsafe { wb_position_sensor_get_value(tag) };
        if value.is_finite() {
            Ok(value)
        } else {
            Err("nonfinite Webots joint measurement".into())
        }
    }
    pub fn effort(&mut self, tag: Tag, linear: bool, value: f64) -> Result<(), String> {
        if !value.is_finite() {
            return Err("nonfinite effort".into());
        }
        // SAFETY: Tag is a generated motor of declared type, thread is owner.
        unsafe {
            if linear {
                wb_motor_set_force(tag, value)
            } else {
                wb_motor_set_torque(tag, value)
            }
        }
        Ok(())
    }
    pub fn sensor(&self, name: &str, kind: &str) -> Result<Tag, String> {
        let tag = self.device(name)?;
        // SAFETY: Names/kinds come from the verified generated device map.
        unsafe {
            match kind {
                "gyro" => wb_gyro_enable(tag, 2),
                "acceleration" => wb_accelerometer_enable(tag, 2),
                "range" => wb_distance_sensor_enable(tag, 2),
                "camera" => wb_camera_enable(tag, 2),
                _ => return Err("unsupported device kind".into()),
            }
        }
        Ok(tag)
    }
    pub fn vector(&self, tag: Tag, acceleration: bool) -> Result<[f64; 3], String> {
        // SAFETY: Enabled vector device owns a 3-double native buffer; copied
        // before the next simulation step, never retained or mutated in Rust.
        unsafe {
            copy(if acceleration {
                wb_accelerometer_get_values(tag)
            } else {
                wb_gyro_get_values(tag)
            })
        }
    }
    pub fn range(&self, tag: Tag) -> Result<f64, String> {
        // SAFETY: Initialized distance-sensor tag with SI identity lookup table.
        let value = unsafe { wb_distance_sensor_get_value(tag) };
        if value.is_finite() {
            Ok(value)
        } else {
            Err("invalid range sample".into())
        }
    }
    pub fn camera(&self, tag: Tag) -> Result<Vec<f64>, String> {
        // SAFETY: Check dimensions and nonnull image before reading the native
        // width*height*4 BGRA buffer. Copy luminance before the next step.
        unsafe {
            if wb_camera_get_width(tag) != 12 || wb_camera_get_height(tag) != 8 {
                return Err("retina dimensions changed".into());
            }
            let ptr = wb_camera_get_image(tag);
            if ptr.is_null() {
                return Err("camera frame unavailable".into());
            }
            let data = std::slice::from_raw_parts(ptr, 12 * 8 * 4);
            Ok(data
                .chunks_exact(4)
                .map(|p| {
                    (0.2126 * f64::from(p[2]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[0]))
                        / 255.
                })
                .collect())
        }
    }
    pub fn node(&self, name: &str) -> Result<Node, String> {
        let name = CString::new(name).map_err(|_| "invalid body name")?;
        // SAFETY: Named generated Solid exists throughout the controller; no
        // node deletion or world reload occurs through this adapter.
        let node = NonNull::new(unsafe { wb_supervisor_node_get_from_def(name.as_ptr()) })
            .ok_or("missing named body")?;
        unsafe {
            wb_supervisor_node_enable_contact_points_tracking(node.as_ptr(), 2, 1);
        }
        Ok(Node(node, PhantomData))
    }
    pub fn body(&self, node: Node) -> Result<BodyState, String> {
        // SAFETY: live generated Solid; copy each fixed native vector.
        unsafe {
            let r: [f64; 9] = copy(wb_supervisor_node_get_orientation(node.0.as_ptr()))?;
            Ok((
                copy(wb_supervisor_node_get_position(node.0.as_ptr()))?,
                std::array::from_fn(|i| std::array::from_fn(|j| r[i * 3 + j])),
                copy(wb_supervisor_node_get_velocity(node.0.as_ptr()))?,
                copy(wb_supervisor_node_get_center_of_mass(node.0.as_ptr()))?,
            ))
        }
    }
    pub fn contact(&self, node: Node) -> bool {
        // SAFETY: live node; descendants reports actual external contacts.
        let mut count = 0;
        unsafe {
            wb_supervisor_node_get_contact_points(node.0.as_ptr(), 1, &mut count);
        }
        count > 0
    }
    pub fn force(
        &mut self,
        node: Node,
        force: [f64; 3],
        local_point: [f64; 3],
    ) -> Result<(), String> {
        if !force
            .iter()
            .chain(local_point.iter())
            .all(|v| v.is_finite())
        {
            return Err("nonfinite aerodynamic input".into());
        }
        // SAFETY: live Solid. API offset is in local node coordinates even
        // when the force is world-relative (relative=false), per R2025a docs.
        unsafe {
            wb_supervisor_node_add_force_with_offset(
                node.0.as_ptr(),
                force.as_ptr(),
                local_point.as_ptr(),
                0,
            );
        }
        Ok(())
    }
    pub fn save(&mut self, node: Node) {
        // SAFETY: live node, static NUL-terminated state label; process-local.
        unsafe {
            wb_supervisor_node_save_state(node.0.as_ptr(), c"initial".as_ptr());
        }
    }
    pub fn reset(&mut self, node: Node) {
        self.disarm();
        // SAFETY: same live Solid and existing process-local saved state.
        unsafe {
            wb_supervisor_node_load_state(node.0.as_ptr(), c"initial".as_ptr());
        }
    }
    pub fn disarm(&mut self) {
        // SAFETY: Every motor tag was validated when added; manual zero effort
        // disables its default position servo. No hidden pose/hold controller.
        unsafe {
            for &(tag, linear) in &self.motors {
                if linear {
                    wb_motor_set_force(tag, 0.)
                } else {
                    wb_motor_set_torque(tag, 0.)
                }
            }
        }
    }
    pub fn quit(&mut self, status: i32) {
        self.disarm();
        unsafe {
            wb_supervisor_simulation_quit(status);
        }
    }
}
// SAFETY: Caller must supply a native buffer of N initialized doubles; all
// callers are documented Webots fixed-vector APIs, never external input.
unsafe fn copy<const N: usize>(ptr: *const f64) -> Result<[f64; N], String> {
    if ptr.is_null() {
        return Err("native measurement unavailable".into());
    }
    let result = std::array::from_fn(|i| unsafe { *ptr.add(i) });
    if !result.iter().all(|v| v.is_finite()) {
        return Err("nonfinite native measurement".into());
    }
    Ok(result)
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.disarm();
        unsafe {
            wb_robot_cleanup();
        }
    }
}
