//! Exclusive model/data ownership for the exact MuJoCo 3.17 ABI.
use std::{
    ffi::{CStr, CString},
    path::Path,
    ptr::NonNull,
};
#[cfg(feature = "viewer")]
#[path = "viewer.rs"]
pub mod viewer;
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_imports
)]
mod ffi {
    include!(concat!(env!("OUT_DIR"), "/mujoco.rs"));
}
/// SI world position, body-to-world rotation and linear/angular velocity.
pub type BodyState = ([f64; 3], [[f64; 3]; 3], [f64; 6]);
/// SI camera position, camera-to-world rotation and native camera index.
pub type CameraState = ([f64; 3], [[f64; 3]; 3], usize);
/// Model and data never escape this owner or cross a thread boundary. Every
/// native call below requires `&mut self` for mutation and binds slices to the
/// owner's lifetime. No raw pointer is exposed to clients or shells.
pub struct Physics {
    model: NonNull<ffi::mjModel>,
    data: NonNull<ffi::mjData>,
    gains: Vec<f64>,
    biases: Vec<f64>,
}
impl Physics {
    pub fn load(path: &Path) -> Result<Self, String> {
        let path = CString::new(path.to_str().ok_or("non-UTF8 model path")?)
            .map_err(|_| "invalid model path")?;
        let mut error = [0 as std::ffi::c_char; 2048];
        // SAFETY: Runtime/header versions are checked before any struct access.
        // CString and error storage remain alive for the call; null VFS selects
        // ordinary filesystem loading. Returned pointers transfer ownership.
        unsafe {
            if ffi::mj_version() != 3_017_000 {
                return Err("native MuJoCo must be exactly 3.17.0".into());
            }
            let model = NonNull::new(ffi::mj_loadXML(
                path.as_ptr(),
                std::ptr::null(),
                error.as_mut_ptr(),
                error.len() as i32,
            ))
            .ok_or_else(|| {
                CStr::from_ptr(error.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            })?;
            let Some(data) = NonNull::new(ffi::mj_makeData(model.as_ptr())) else {
                ffi::mj_deleteModel(model.as_ptr());
                return Err("MuJoCo data allocation failed".into());
            };
            let count = model.as_ref().nu as usize;
            // SAFETY: MuJoCo allocates 10 coefficients for each actuator row.
            let gains =
                std::slice::from_raw_parts(model.as_ref().actuator_gainprm, count * 10).to_vec();
            let biases =
                std::slice::from_raw_parts(model.as_ref().actuator_biasprm, count * 10).to_vec();
            let mut owner = Self {
                model,
                data,
                gains,
                biases,
            };
            owner.forward();
            Ok(owner)
        }
    }
    fn m(&self) -> &ffi::mjModel {
        // SAFETY: owned live allocation; immutable borrow.
        unsafe { self.model.as_ref() }
    }
    fn d(&self) -> &ffi::mjData {
        // SAFETY: owned live allocation; immutable borrow.
        unsafe { self.data.as_ref() }
    }
    pub fn step(&mut self) {
        // SAFETY: model/data correspond; exclusive data owner.
        unsafe {
            ffi::mj_step(self.model.as_ptr(), self.data.as_ptr());
        }
    }
    pub fn forward(&mut self) {
        // SAFETY: matching allocations, exclusive borrow.
        unsafe {
            ffi::mj_forward(self.model.as_ptr(), self.data.as_ptr());
        }
    }
    pub fn reset(&mut self) {
        // SAFETY: matching allocations, no live mutable slices.
        unsafe {
            ffi::mj_resetData(self.model.as_ptr(), self.data.as_ptr());
        }
        self.forward();
    }
    pub fn time(&self) -> f64 {
        self.d().time
    }
    pub fn dimensions(&self) -> (usize, usize, usize) {
        (
            self.m().nq as usize,
            self.m().nv as usize,
            self.m().nu as usize,
        )
    }
    pub fn qpos(&self) -> &[f64] {
        // SAFETY: MuJoCo allocates nq scalars until drop.
        unsafe { std::slice::from_raw_parts(self.d().qpos, self.m().nq as usize) }
    }
    pub fn qvel(&self) -> &[f64] {
        // SAFETY: MuJoCo allocates nv scalars until drop.
        unsafe { std::slice::from_raw_parts(self.d().qvel, self.m().nv as usize) }
    }
    pub fn actuator(&self, name: &str) -> Result<usize, String> {
        self.named(ffi::mjtObj_mjOBJ_ACTUATOR as i32, name)
    }
    pub fn joint(&self, name: &str) -> Result<(usize, usize), String> {
        let j = self.named(ffi::mjtObj_mjOBJ_JOINT as i32, name)?;
        // SAFETY: named ID was range-checked by MuJoCo; arrays have njnt values.
        unsafe {
            Ok((
                *self.m().jnt_qposadr.add(j) as usize,
                *self.m().jnt_dofadr.add(j) as usize,
            ))
        }
    }
    fn named(&self, kind: i32, name: &str) -> Result<usize, String> {
        let c = CString::new(name).map_err(|_| "invalid element name")?;
        // SAFETY: model and name remain alive for the call. No borrowed result.
        let id = unsafe { ffi::mj_name2id(self.model.as_ptr(), kind, c.as_ptr()) };
        if id < 0 {
            Err(format!("missing named model element {name}"))
        } else {
            Ok(id as usize)
        }
    }
    pub fn set_control(&mut self, index: usize, value: f64) -> Result<(), String> {
        if index >= self.m().nu as usize || !value.is_finite() {
            return Err("invalid control".into());
        }
        // SAFETY: index is within nu, value finite, exclusive data ownership.
        unsafe {
            *self.d().ctrl.add(index) = value;
        }
        Ok(())
    }
    pub fn control(&mut self, index: usize, value: f64, active: bool) -> Result<(), String> {
        self.control_with_gain(index, value, active, 1.)
    }
    pub fn control_with_gain(
        &mut self,
        index: usize,
        value: f64,
        active: bool,
        scale: f64,
    ) -> Result<(), String> {
        if !scale.is_finite() || !(1. ..=5.).contains(&scale) {
            return Err("invalid muscle stiffness gain".into());
        }
        self.set_control(index, value)?;
        // SAFETY: set_control validated the actuator index. Saved coefficient
        // arrays were copied from this model and have exactly nu*10 entries.
        unsafe {
            for j in 0..10 {
                *self.m().actuator_gainprm.add(index * 10 + j) = if active {
                    self.gains[index * 10 + j] * scale
                } else {
                    0.
                };
                *self.m().actuator_biasprm.add(index * 10 + j) = if active {
                    self.biases[index * 10 + j] * scale
                } else {
                    0.
                };
            }
        }
        Ok(())
    }
    pub fn body(&self, name: &str) -> Result<usize, String> {
        self.named(ffi::mjtObj_mjOBJ_BODY as i32, name)
    }
    pub fn body_state(&self, index: usize) -> Result<BodyState, String> {
        if index >= self.m().nbody as usize {
            return Err("body index outside model".into());
        }
        // SAFETY: body arrays have nbody rows and native output storage is six
        // initialized scalars. IDs are validated; returned arrays own copies.
        unsafe {
            let position = std::array::from_fn(|i| *self.d().xpos.add(index * 3 + i));
            let rotation = std::array::from_fn(|i| {
                std::array::from_fn(|j| *self.d().xmat.add(index * 9 + i * 3 + j))
            });
            let mut velocity = [0.; 6];
            ffi::mj_objectVelocity(
                self.model.as_ptr(),
                self.data.as_ptr(),
                ffi::mjtObj_mjOBJ_BODY as i32,
                index as i32,
                velocity.as_mut_ptr(),
                0,
            );
            Ok((position, rotation, velocity))
        }
    }
    pub fn clear_forces(&mut self) {
        // SAFETY: qfrc_applied contains nv scalars; exclusive owner.
        unsafe {
            for i in 0..self.m().nv as usize {
                *self.d().qfrc_applied.add(i) = 0.;
            }
        }
    }
    pub fn force_at(
        &mut self,
        body: usize,
        force: [f64; 3],
        point: [f64; 3],
    ) -> Result<(), String> {
        if body >= self.m().nbody as usize || !force.into_iter().chain(point).all(f64::is_finite) {
            return Err("invalid applied force".into());
        }
        // SAFETY: valid body, fixed-size finite arrays, matching model/data and
        // exclusively owned nv-element generalized force destination.
        unsafe {
            ffi::mj_applyFT(
                self.model.as_ptr(),
                self.data.as_ptr(),
                force.as_ptr(),
                [0.; 3].as_ptr(),
                point.as_ptr(),
                body as i32,
                self.d().qfrc_applied,
            );
        }
        Ok(())
    }
    pub fn ray(
        &self,
        point: [f64; 3],
        direction: [f64; 3],
        exclude: usize,
    ) -> Result<Option<f64>, String> {
        if exclude >= self.m().nbody as usize
            || !point.into_iter().chain(direction).all(f64::is_finite)
        {
            return Err("invalid ray".into());
        }
        let mut geom = -1;
        // Range sensors exclude massless anatomical/scene decoration (group 2).
        // Retinal rays below still see the full rendered geometry.
        let groups: [u8; 6] = [1, 1, 0, 1, 1, 1];
        // SAFETY: fixed arrays and valid model; group mask has six elements.
        let distance = unsafe {
            ffi::mj_ray(
                self.model.as_ptr(),
                self.data.as_ptr(),
                point.as_ptr(),
                direction.as_ptr(),
                groups.as_ptr(),
                true,
                exclude as i32,
                &mut geom,
                std::ptr::null_mut(),
            )
        };
        Ok((distance >= 0.).then_some(distance))
    }
    pub fn camera_state(&self, name: &str) -> Result<CameraState, String> {
        let index = self.named(ffi::mjtObj_mjOBJ_CAMERA as i32, name)?;
        // SAFETY: validated camera ID, fixed native camera/body array rows.
        unsafe {
            Ok((
                std::array::from_fn(|i| *self.d().cam_xpos.add(index * 3 + i)),
                std::array::from_fn(|i| {
                    std::array::from_fn(|j| *self.d().cam_xmat.add(index * 9 + i * 3 + j))
                }),
                *self.m().cam_bodyid.add(index) as usize,
            ))
        }
    }
    pub fn ray_light(
        &self,
        point: [f64; 3],
        direction: [f64; 3],
        exclude: usize,
    ) -> Result<f64, String> {
        if exclude >= self.m().nbody as usize
            || !point.into_iter().chain(direction).all(f64::is_finite)
        {
            return Err("invalid retinal ray".into());
        }
        let mut geom = -1;
        // SAFETY: fixed-size finite arrays; output is a single native geom ID.
        unsafe {
            let distance = ffi::mj_ray(
                self.model.as_ptr(),
                self.data.as_ptr(),
                point.as_ptr(),
                direction.as_ptr(),
                std::ptr::null(),
                true,
                exclude as i32,
                &mut geom,
                std::ptr::null_mut(),
            );
            if distance < 0. || geom < 0 {
                return Ok(0.1);
            }
            if i64::from(geom) >= self.m().ngeom {
                return Err("invalid ray geom".into());
            }
            Ok((0..3)
                .map(|i| {
                    f64::from(*self.m().geom_rgba.add(geom as usize * 4 + i))
                        * [0.2126, 0.7152, 0.0722][i]
                })
                .sum())
        }
    }
    pub fn contact_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        // SAFETY: MuJoCo exposes ncon initialized contacts; each geom index is
        // checked before resolving its borrowed name, copied before return.
        unsafe {
            for i in 0..self.d().ncon as usize {
                let c = &*self.d().contact.add(i);
                for geom in [c.geom1, c.geom2] {
                    if geom >= 0 && i64::from(geom) < self.m().ngeom {
                        let name = ffi::mj_id2name(
                            self.model.as_ptr(),
                            ffi::mjtObj_mjOBJ_GEOM as i32,
                            geom,
                        );
                        if !name.is_null() {
                            names.push(CStr::from_ptr(name).to_string_lossy().into_owned());
                        }
                    }
                }
            }
        }
        names
    }
    /// Native bumper measurement including ground, unlike worm object touch.
    pub fn body_contact(&self, body: usize) -> Result<f64, String> {
        if body >= self.m().nbody as usize {
            return Err("contact body index".into());
        }
        // SAFETY: native contact and geom tables belong to this live owner.
        unsafe {
            for i in 0..self.d().ncon as usize {
                let c = &*self.d().contact.add(i);
                for g in [c.geom1, c.geom2] {
                    if g >= 0
                        && (g as usize) < self.m().ngeom as usize
                        && *self.m().geom_bodyid.add(g as usize) == body as i32
                    {
                        return Ok(1.);
                    }
                }
            }
        }
        Ok(0.)
    }
    /// Sum actual constraint contact forces, rotated to the named body frame.
    pub fn contact_force(&self, body: usize) -> Result<[f64; 3], String> {
        let (_, r, _) = self.body_state(body)?;
        let mut world = [0.; 3];
        // SAFETY: indices and six-value scratch buffer follow mj_contactForce.
        unsafe {
            for i in 0..self.d().ncon as usize {
                let c = &*self.d().contact.add(i);
                let mut sign = 0.;
                for (g, s) in [(c.geom1, -1.), (c.geom2, 1.)] {
                    if g >= 0
                        && (g as usize) < self.m().ngeom as usize
                        && *self.m().geom_bodyid.add(g as usize) == body as i32
                    {
                        sign += s;
                    }
                }
                if sign == 0. {
                    continue;
                }
                let mut f = [0.; 6];
                ffi::mj_contactForce(
                    self.model.as_ptr(),
                    self.data.as_ptr(),
                    i as i32,
                    f.as_mut_ptr(),
                );
                for (axis, w) in world.iter_mut().enumerate() {
                    *w += sign * (0..3).map(|j| c.frame[3 * j + axis] * f[j]).sum::<f64>();
                }
            }
        }
        Ok(std::array::from_fn(|i| {
            (0..3).map(|j| r[j][i] * world[j]).sum()
        }))
    }
    pub fn body_mass(&self, body: usize) -> Result<f64, String> {
        if body >= self.m().nbody as usize {
            return Err("body mass index".into());
        }
        // SAFETY: checked body index in this model's immutable mass array.
        Ok(unsafe { *self.m().body_mass.add(body) })
    }
    /// Classify actual contact positions against a declared body-local bumper
    /// half-space. Reduced pads share a rigid owner, not a fabricated value.
    pub fn directional_contact(&self, body: usize, direction: [f64; 3]) -> Result<f64, String> {
        let (position, r, _) = self.body_state(body)?;
        if !direction.into_iter().all(f64::is_finite) {
            return Err("invalid bumper direction".into());
        }
        // SAFETY: owner-validated body; native contact/geom arrays are live.
        unsafe {
            for i in 0..self.d().ncon as usize {
                let c = &*self.d().contact.add(i);
                let belongs = [c.geom1, c.geom2].iter().any(|g| {
                    *g >= 0
                        && (*g as usize) < self.m().ngeom as usize
                        && *self.m().geom_bodyid.add(*g as usize) == body as i32
                });
                if belongs {
                    let local: [f64; 3] = std::array::from_fn(|axis| {
                        (0..3).map(|j| r[j][axis] * (c.pos[j] - position[j])).sum()
                    });
                    if (0..3).map(|j| local[j] * direction[j]).sum::<f64>() > 0. {
                        return Ok(1.);
                    }
                }
            }
        }
        Ok(0.)
    }
    pub fn body_count(&self) -> usize {
        self.m().nbody as usize
    }
    pub fn camera_fovy(&self, name: &str) -> Result<f64, String> {
        let index = self.named(ffi::mjtObj_mjOBJ_CAMERA as i32, name)?;
        // SAFETY: validated native camera index, immutable optics table.
        Ok(unsafe { *self.m().cam_fovy.add(index) })
    }
    /// Bumper proxy excludes the agar/ground and the body's own segments.
    pub fn object_contact(&self, body: usize) -> f64 {
        // SAFETY: all contact/geom IDs originate in this live model; checked.
        unsafe {
            for i in 0..self.d().ncon as usize {
                let c = &*self.d().contact.add(i);
                if c.geom1 < 0 || c.geom2 < 0 {
                    continue;
                }
                let a = *self.m().geom_bodyid.add(c.geom1 as usize) as usize;
                let b = *self.m().geom_bodyid.add(c.geom2 as usize) as usize;
                if a != body && b != body {
                    continue;
                }
                let other = if a == body { c.geom2 } else { c.geom1 };
                let name =
                    ffi::mj_id2name(self.model.as_ptr(), ffi::mjtObj_mjOBJ_GEOM as i32, other);
                let name = if name.is_null() {
                    ""
                } else {
                    CStr::from_ptr(name).to_str().unwrap_or("")
                };
                if !["ground", "floor", "platform"].contains(&name) && !name.starts_with("segment_")
                {
                    return 1.;
                }
            }
        }
        0.
    }
    /// Ideal renderer-light proxy: normalized diffuse intensity with declared
    /// 1/(1+r²) attenuation. Warm channel uses the lamp's red component.
    pub fn irradiance_proxy(&self, point: [f64; 3], warm: bool) -> f64 {
        // SAFETY: light arrays have nlight rows of three elements.
        unsafe {
            (0..self.m().nlight as usize)
                .map(|i| {
                    let distance2 = (0..3)
                        .map(|j| (point[j] - *self.d().light_xpos.add(3 * i + j)).powi(2))
                        .sum::<f64>();
                    let intensity = if warm {
                        f64::from(*self.m().light_diffuse.add(3 * i))
                    } else {
                        (0..3)
                            .map(|j| f64::from(*self.m().light_diffuse.add(3 * i + j)))
                            .sum::<f64>()
                            / 3.
                    };
                    intensity / (1. + distance2)
                })
                .sum::<f64>()
                .clamp(0., 1.)
        }
    }
    pub fn body_acceleration(&mut self, index: usize) -> Result<[f64; 6], String> {
        if index >= self.m().nbody as usize {
            return Err("body index outside model".into());
        }
        let mut value = [0.; 6];
        // SAFETY: valid ID; output spans six initialized scalars.
        unsafe {
            ffi::mj_rnePostConstraint(self.model.as_ptr(), self.data.as_ptr());
            ffi::mj_objectAcceleration(
                self.model.as_ptr(),
                self.data.as_ptr(),
                ffi::mjtObj_mjOBJ_BODY as i32,
                index as i32,
                value.as_mut_ptr(),
                0,
            );
        }
        // mjOBJ_BODY with local=1 uses MuJoCo's inertial principal-axis frame,
        // which can rotate when a box becomes a capsule. Project the world
        // result explicitly into the named body's frame used by our IO map.
        let (_, rotation, _) = self.body_state(index)?;
        Ok(std::array::from_fn(|i| {
            let offset = if i < 3 { 0 } else { 3 };
            (0..3).map(|j| rotation[j][i % 3] * value[offset + j]).sum()
        }))
    }
    pub fn center_of_mass(&self, index: usize) -> Result<[f64; 3], String> {
        if index >= self.m().nbody as usize {
            return Err("body index outside model".into());
        }
        // SAFETY: xipos has nbody rows; return an owned scalar array.
        unsafe { Ok(std::array::from_fn(|i| *self.d().xipos.add(index * 3 + i))) }
    }
    pub fn mechanical_power(&self) -> f64 {
        // SAFETY: both native arrays contain nu scalars and remain live.
        unsafe {
            (0..self.m().nu as usize)
                .map(|i| {
                    (*self.d().actuator_force.add(i) * *self.d().actuator_velocity.add(i)).abs()
                })
                .sum()
        }
    }
    pub fn has_active_effort(&self) -> bool {
        // SAFETY: MuJoCo actuator_force contains nu initialized scalars.
        unsafe { (0..self.m().nu as usize).any(|i| (*self.d().actuator_force.add(i)).abs() > 1e-9) }
    }
    pub fn predicted_power(&self) -> f64 {
        // SAFETY: both arrays have nu entries. actuator_velocity already
        // includes the transmission gear; do not apply that factor twice.
        unsafe {
            (0..self.m().nu as usize)
                .map(|i| (*self.d().ctrl.add(i) * *self.d().actuator_velocity.add(i)).abs())
                .sum()
        }
    }
    pub fn scale_controls(&mut self, factor: f64) -> Result<(), String> {
        if !factor.is_finite() || !(0. ..=1.).contains(&factor) {
            return Err("invalid power scaling".into());
        }
        // SAFETY: exclusive owner of the nu-element control array.
        unsafe {
            for i in 0..self.m().nu as usize {
                *self.d().ctrl.add(i) *= factor;
            }
        }
        Ok(())
    }
    pub fn poses(&self) -> Vec<serde_json::Value> {
        // SAFETY: IDs are bounded by nbody; names/poses are copied before return.
        unsafe {
            (1..self.m().nbody as usize).filter_map(|index| {
                let name = ffi::mj_id2name(self.model.as_ptr(), ffi::mjtObj_mjOBJ_BODY as i32, index as i32);
                if name.is_null() { return None; }
                let position: [f64;3] = std::array::from_fn(|i| *self.d().xpos.add(index*3+i));
                let quaternion: [f64;4] = std::array::from_fn(|i| *self.d().xquat.add(index*4+i));
                Some(serde_json::json!({"name":CStr::from_ptr(name).to_string_lossy(),"position":position,"quaternion_wxyz":quaternion}))
            }).collect()
        }
    }
    pub fn place_voxel(&mut self, slot: usize, position: [f64; 3]) -> Result<(), String> {
        if slot >= 32 || !position.iter().all(|x| x.is_finite() && x.abs() <= 10.) {
            return Err("voxel position/slot budget".into());
        }
        let body = self.body(&format!("world_voxel_{slot}"))?;
        let (root, _, _) = self.body_state(1)?;
        if position
            .iter()
            .zip(root)
            .map(|(p, r)| (p - r).powi(2))
            .sum::<f64>()
            < 0.9f64.powi(2)
        {
            return Err("obstacle overlaps body safety envelope".into());
        }
        // SAFETY: valid body ID; mocap ID checked against nmocap before write.
        unsafe {
            let id = *self.m().body_mocapid.add(body);
            if id < 0 || id as i64 >= self.m().nmocap {
                return Err("world has no collision mirror".into());
            }
            for (i, value) in position.into_iter().enumerate() {
                *self.d().mocap_pos.add(id as usize * 3 + i) = value;
            }
        }
        self.forward();
        Ok(())
    }
    /// Replace the bounded unit-cube scene at one owner boundary. Validate all
    /// positions and native handles before any write; omitted slots deactivate.
    pub fn replace_voxels(&mut self, positions: &[[f64; 3]]) -> Result<(), String> {
        if positions.len() > 32 {
            return Err("collision mirror capacity exceeded".into());
        }
        let (root, _, _) = self.body_state(1)?;
        for p in positions {
            if !p.iter().all(|x| x.is_finite() && x.abs() <= 10.)
                || p.iter()
                    .zip(root)
                    .map(|(p, r)| (p - r).powi(2))
                    .sum::<f64>()
                    < 0.9f64.powi(2)
            {
                return Err("invalid or overlapping collision mirror".into());
            }
        }
        let mut ids = Vec::with_capacity(32);
        for slot in 0..32 {
            let body = self.body(&format!("world_voxel_{slot}"))?;
            // SAFETY: named body IDs were checked; model/data are exclusively owned.
            let id = unsafe { *self.m().body_mocapid.add(body) };
            if id < 0 || id as i64 >= self.m().nmocap {
                return Err("world has no collision mirror".into());
            }
            ids.push(id as usize);
        }
        for (slot, id) in ids.into_iter().enumerate() {
            let p = positions
                .get(slot)
                .copied()
                .unwrap_or([100. + slot as f64 * 2., 0., -100.]);
            for (i, value) in p.into_iter().enumerate() {
                // SAFETY: all mocap indices validated above; no fallible mutation.
                unsafe {
                    *self.d().mocap_pos.add(id * 3 + i) = value;
                }
            }
        }
        self.forward();
        Ok(())
    }
    pub fn snapshot(&self) -> Vec<f64> {
        // SAFETY: native size exactly determines the initialized output span.
        unsafe {
            let spec = ffi::mjtState_mjSTATE_INTEGRATION as i32;
            let mut out = vec![0.; ffi::mj_stateSize(self.model.as_ptr(), spec) as usize];
            ffi::mj_getState(
                self.model.as_ptr(),
                self.data.as_ptr(),
                out.as_mut_ptr(),
                spec,
            );
            out
        }
    }
    pub fn restore(&mut self, state: &[f64]) -> Result<(), String> {
        // SAFETY: exact size/finiteness checked before native read; matching
        // owned allocations; restored effects are disarmed, never replayed.
        unsafe {
            let spec = ffi::mjtState_mjSTATE_INTEGRATION as i32;
            if state.len() != ffi::mj_stateSize(self.model.as_ptr(), spec) as usize
                || !state.iter().all(|x| x.is_finite())
            {
                return Err("invalid integration snapshot".into());
            }
            ffi::mj_setState(
                self.model.as_ptr(),
                self.data.as_ptr(),
                state.as_ptr(),
                spec,
            );
        }
        self.disarm();
        self.forward();
        Ok(())
    }
    pub fn disarm(&mut self) {
        // SAFETY: arrays contain nu fixed-size rows; exclusively owned model.
        unsafe {
            for i in 0..self.m().nu as usize {
                *self.d().ctrl.add(i) = 0.;
                for j in 0..10 {
                    *self.m().actuator_gainprm.add(i * 10 + j) = 0.;
                    *self.m().actuator_biasprm.add(i * 10 + j) = 0.;
                }
            }
        }
    }
}
impl Drop for Physics {
    fn drop(&mut self) {
        // SAFETY: delete each allocation exactly once, data before model.
        unsafe {
            ffi::mj_deleteData(self.data.as_ptr());
            ffi::mj_deleteModel(self.model.as_ptr());
        }
    }
}
