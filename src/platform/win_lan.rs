//! Windows adapter `OperStatus`, keyed the same way `default_net` names interfaces.
//! Address selection stays in `lan::interfaces_from_default_net`.

use std::collections::HashMap;

#[derive(Default)]
pub struct AdapterOperStatus {
    by_name: HashMap<String, u32>,
    by_index: HashMap<u32, u32>,
}

impl AdapterOperStatus {
    pub fn get(&self, name: &str, index: u32) -> Option<u32> {
        if !name.is_empty() {
            if let Some(status) = self.by_name.get(&name.to_ascii_lowercase()) {
                return Some(*status);
            }
        }
        if index != 0 {
            return self.by_index.get(&index).copied();
        }
        None
    }
}

pub fn adapter_oper_status() -> Option<AdapterOperStatus> {
    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA, NO_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, GAA_FLAG_SKIP_UNICAST,
    };

    let flags = GAA_FLAG_SKIP_UNICAST
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER;
    let mut needed = 16 * 1024u32;
    for _ in 0..3 {
        let words = (needed as usize).div_ceil(std::mem::size_of::<u64>()) + 1;
        let mut buf = vec![0u64; words];
        let mut size = (buf.len() * std::mem::size_of::<u64>()) as u32;
        let ret = unsafe {
            GetAdaptersAddresses(0, flags, None, Some(buf.as_mut_ptr().cast()), &mut size)
        };
        if ret == NO_ERROR.0 {
            let mut status = AdapterOperStatus::default();
            unsafe { read_oper_status(&buf, &mut status) };
            return Some(status);
        }
        if ret == ERROR_NO_DATA.0 {
            return Some(AdapterOperStatus::default());
        }
        if ret != ERROR_BUFFER_OVERFLOW.0 || size == 0 || size > 16 * 1024 * 1024 {
            return None;
        }
        needed = size;
    }
    None
}

unsafe fn read_oper_status(buf: &[u64], out: &mut AdapterOperStatus) {
    use windows::Win32::NetworkManagement::IpHelper::IP_ADAPTER_ADDRESSES_LH;

    let mut cur = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    let mut seen = 0u32;
    while !cur.is_null() && seen < 512 {
        seen += 1;
        let row = &*cur;
        let oper = row.OperStatus.0 as u32;
        let index = row.Anonymous1.Anonymous.IfIndex;
        if index != 0 {
            out.by_index.insert(index, oper);
        }
        if !row.AdapterName.is_null() {
            let bytes = row.AdapterName.as_bytes();
            if !bytes.is_empty() {
                out.by_name
                    .insert(String::from_utf8_lossy(bytes).to_ascii_lowercase(), oper);
            }
        }
        cur = row.Next;
    }
}
