use std::os::raw::c_char;

extern "C" {
    fn TcQR(value: *const c_char) -> *mut c_char;
    fn TcFree(value: *mut c_char);
}

#[no_mangle]
pub extern "C" fn tailcat_link_probe() -> i32 {
    let response = unsafe { TcQR(b"TailDesk Android link probe\0".as_ptr().cast()) };
    if response.is_null() {
        return 1;
    }
    let empty = unsafe { *response == 0 };
    unsafe { TcFree(response) };
    i32::from(empty)
}
