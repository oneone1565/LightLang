use sha2::{Digest, Sha256};
use std::os::raw::c_void;
use std::slice;

#[no_mangle]
pub extern "C" fn light_sha256_prefix32(s: *const c_void) -> i64 {
    if s.is_null() {
        return 0;
    }
    let (ptr, len) = lightrt::str_parts(s);
    if ptr.is_null() {
        return 0;
    }
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    let digest = Sha256::digest(bytes);
    i64::from(u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]))
}
