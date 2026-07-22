use crate::abi::{decode_canonical, encode_canonical, pack_ptr_len, ConnectorEvent};
use crate::{Connector, ConnectorError};
use std::sync::{Mutex, MutexGuard, TryLockError};

pub struct RuntimeCell<C> {
    connector: Mutex<Option<C>>,
}

impl<C> RuntimeCell<C> {
    pub const fn new() -> Self {
        Self {
            connector: Mutex::new(None),
        }
    }
}

impl<C> Default for RuntimeCell<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Connector> RuntimeCell<C> {
    fn lock(&self) -> Result<MutexGuard<'_, Option<C>>, ConnectorError> {
        match self.connector.try_lock() {
            Ok(guard) => Ok(guard),
            Err(TryLockError::Poisoned(poisoned)) => Ok(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => Err(ConnectorError::ReentrantCall),
        }
    }

    fn initialize(&self, event: ConnectorEvent) -> Result<Vec<u8>, ConnectorError> {
        let mut guard = self.lock()?;
        let mut connector = C::default();
        let batch = connector.init(event)?;
        *guard = Some(connector);
        encode_canonical(&batch).map_err(Into::into)
    }

    fn handle(&self, event: ConnectorEvent) -> Result<Vec<u8>, ConnectorError> {
        let mut guard = self.lock()?;
        let connector = guard.get_or_insert_with(C::default);
        let batch = connector.handle(event)?;
        encode_canonical(&batch).map_err(Into::into)
    }

    fn snapshot(&self) -> Result<Vec<u8>, ConnectorError> {
        let guard = self.lock()?;
        match guard.as_ref() {
            Some(connector) => connector.snapshot(),
            None => C::default().snapshot(),
        }
    }
}

pub fn ffi_alloc(len: i32) -> i32 {
    let Ok(len) = usize::try_from(len) else {
        return 0;
    };
    if len == 0 {
        return 0;
    }
    let bytes = vec![0_u8; len].into_boxed_slice();
    Box::into_raw(bytes).cast::<u8>() as usize as i32
}

pub fn ffi_dealloc(ptr: i32, len: i32) {
    let (Ok(pointer), Ok(length)) = (usize::try_from(ptr), usize::try_from(len)) else {
        return;
    };
    if pointer == 0 || length == 0 {
        return;
    }
    let slice = std::ptr::slice_from_raw_parts_mut(pointer as *mut u8, length);
    // SAFETY: the ABI requires ptr/len to be a live allocation returned by ffi_alloc or an output
    // helper, used exactly once for deallocation. The host validates this contract before calling.
    unsafe {
        drop(Box::from_raw(slice));
    }
}

pub fn ffi_init<C: Connector>(cell: &RuntimeCell<C>, ptr: i32, len: i32) -> i64 {
    call_with_event(ptr, len, |event| cell.initialize(event))
}

pub fn ffi_handle<C: Connector>(cell: &RuntimeCell<C>, ptr: i32, len: i32) -> i64 {
    call_with_event(ptr, len, |event| cell.handle(event))
}

/// Zero is a legal empty snapshot; -1 says the guest could not build one. Collapsing the two would
/// turn a failed snapshot into empty state, which the host would then persist as the truth.
pub fn ffi_snapshot<C: Connector>(cell: &RuntimeCell<C>) -> i64 {
    cell.snapshot().map_or(-1, return_bytes)
}

fn call_with_event(
    ptr: i32,
    len: i32,
    call: impl FnOnce(ConnectorEvent) -> Result<Vec<u8>, ConnectorError>,
) -> i64 {
    let (Ok(pointer), Ok(length)) = (usize::try_from(ptr), usize::try_from(len)) else {
        return 0;
    };
    if pointer == 0 || length == 0 {
        return 0;
    }
    // SAFETY: the host passes a live allocation produced by ffi_alloc and keeps it alive for this
    // call. The slice is immutable and never retained across the call.
    let input = unsafe { std::slice::from_raw_parts(pointer as *const u8, length) };
    let Ok(event) = decode_canonical(input) else {
        return 0;
    };
    call(event).map_or(0, return_bytes)
}

fn return_bytes(bytes: Vec<u8>) -> i64 {
    if bytes.is_empty() || bytes.len() > u32::MAX as usize {
        return 0;
    }
    let boxed = bytes.into_boxed_slice();
    let len = boxed.len() as u32;
    let pointer = Box::into_raw(boxed).cast::<u8>() as usize;
    if pointer > u32::MAX as usize {
        return 0;
    }
    pack_ptr_len(pointer as u32, len)
}
