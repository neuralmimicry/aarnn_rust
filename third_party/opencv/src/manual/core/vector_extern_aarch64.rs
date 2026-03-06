#![cfg(target_arch = "aarch64")]
#![allow(non_snake_case)]

use std::ffi::c_void;

use crate::core;
use crate::platform_types::size_t;
use crate::traits::{Boxed, OpenCVFromExtern, OpenCVIntoExternContainer, OpenCVType};
use crate::{extern_arg_send, extern_container_send, extern_receive};

fn missing_vector_extern(typ: &str) -> ! {
	panic!("OpenCV vector extern is not available for {typ} on this aarch64 build")
}

macro_rules! impl_missing_vector_extern {
	($typ:ty) => {
		impl core::VectorExtern<$typ> for core::Vector<$typ> {
			unsafe fn extern_new() -> extern_receive!(Self)
			where
				Self: OpenCVFromExtern,
			{
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_delete(&mut self) {
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_len(&self) -> size_t {
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_is_empty(&self) -> bool {
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_capacity(&self) -> size_t {
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_shrink_to_fit(&mut self) {
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_reserve(&mut self, _additional: size_t) {
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_remove(&mut self, _index: size_t) {
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_swap(&mut self, _index1: size_t, _index2: size_t) {
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_clear(&mut self) {
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_get(&self, _index: size_t) -> extern_receive!($typ)
			where
				$typ: OpenCVFromExtern,
			{
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_push(&mut self, _val: extern_arg_send!($typ: '_))
			where
				$typ: for<'t> OpenCVType<'t>,
			{
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_push_owned(&mut self, _val: extern_container_send!($typ))
			where
				$typ: OpenCVIntoExternContainer,
			{
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_insert(&mut self, _index: size_t, _val: extern_arg_send!($typ: '_))
			where
				$typ: for<'t> OpenCVType<'t>,
			{
				missing_vector_extern(stringify!($typ))
			}

			unsafe fn extern_set(&mut self, _index: size_t, _val: extern_arg_send!($typ: '_))
			where
				$typ: for<'t> OpenCVType<'t>,
			{
				missing_vector_extern(stringify!($typ))
			}
		}
	};
}

impl_missing_vector_extern!(bool);
impl_missing_vector_extern!(f64);
impl_missing_vector_extern!(crate::videoio::VideoCapture);
impl_missing_vector_extern!(core::Range);
impl_missing_vector_extern!(core::UMat);

// Some arm64 OpenCV builds generate APIs that accept Vector<T> in signatures
// without generating the usual helper methods in types.rs.
impl core::Vector<bool> {
	pub fn as_raw_VectorOfbool(&self) -> *const c_void { self.as_raw() }
	pub fn as_raw_mut_VectorOfbool(&mut self) -> *mut c_void { self.as_raw_mut() }
}

impl core::Vector<f64> {
	pub fn as_raw_VectorOff64(&self) -> *const c_void { self.as_raw() }
	pub fn as_raw_mut_VectorOff64(&mut self) -> *mut c_void { self.as_raw_mut() }
}

impl core::Vector<crate::videoio::VideoCapture> {
	pub fn as_raw_VectorOfVideoCapture(&self) -> *const c_void { self.as_raw() }
	pub fn as_raw_mut_VectorOfVideoCapture(&mut self) -> *mut c_void { self.as_raw_mut() }
}

impl core::Vector<core::Range> {
	pub fn as_raw_VectorOfRange(&self) -> *const c_void { self.as_raw() }
	pub fn as_raw_mut_VectorOfRange(&mut self) -> *mut c_void { self.as_raw_mut() }
}

impl core::Vector<core::UMat> {
	pub fn as_raw_VectorOfUMat(&self) -> *const c_void { self.as_raw() }
	pub fn as_raw_mut_VectorOfUMat(&mut self) -> *mut c_void { self.as_raw_mut() }
}
