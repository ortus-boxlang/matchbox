//! OS byte streams only. Encoding, protocols and application policy stay in BoxLang.
#![cfg(unix)]
use matchbox_plugin_sdk::*;
use std::{
    ffi::c_void,
    fs::File,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream, ToSocketAddrs},
    os::fd::{AsRawFd, FromRawFd, RawFd},
    sync::OnceLock,
    time::{Duration, Instant},
};

static BYTES: OnceLock<&'static BytesV1> = OnceLock::new();
const KIND: &str = "bx-matchbox-std.resource";
const MAX_READ: usize = 1024 * 1024;

enum Resource {
    Listener(TcpListener),
    Socket(TcpStream),
    Input(File),
    Output(File),
}
struct Handle {
    resource: Option<Resource>,
    buffer: Vec<u8>,
}
enum Fill {
    Data,
    Timeout,
    Eof,
}
impl Handle {
    fn fill(&mut self, until: Option<Instant>, timed: bool) -> Result<Fill, String> {
        let resource = self.resource.as_mut().ok_or("Stream is closed")?;
        if !matches!(resource, Resource::Socket(_) | Resource::Input(_)) {
            return Err("Stream is not readable".into());
        }
        let mut bytes = [0; 8192];
        loop {
            if timed && until.is_some_and(|until| Instant::now() >= until) {
                return Ok(Fill::Timeout);
            }
            if !wait(resource.fd(), libc::POLLIN, remaining(until))
                .map_err(|error| error.to_string())?
            {
                return Ok(Fill::Timeout);
            }
            let result = match resource {
                Resource::Socket(stream) => stream.read(&mut bytes),
                Resource::Input(file) => file.read(&mut bytes),
                _ => unreachable!(),
            };
            match result {
                Ok(0) => return Ok(Fill::Eof),
                Ok(count) => {
                    self.buffer.extend_from_slice(&bytes[..count]);
                    return Ok(Fill::Data);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    }
    fn take(&mut self, count: usize) -> Vec<u8> {
        self.buffer.drain(..count).collect()
    }
}
impl Resource {
    fn fd(&self) -> RawFd {
        match self {
            Self::Listener(value) => value.as_raw_fd(),
            Self::Socket(value) => value.as_raw_fd(),
            Self::Input(value) | Self::Output(value) => value.as_raw_fd(),
        }
    }
}
fn integer(call: &Call<'_>, index: usize, min: i64, max: i64) -> Result<i64, String> {
    let number = call.number(call.argument(index)?)?;
    if !number.is_finite() || number.fract() != 0.0 || number < min as f64 || number > max as f64 {
        return Err(format!(
            "Argument {} must be an integer between {min} and {max}",
            index + 1
        ));
    }
    Ok(number as i64)
}
fn timeout(call: &Call<'_>, index: usize) -> Result<i64, String> {
    integer(call, index, -1, i32::MAX as i64)
}
fn deadline(ms: i64) -> Option<Instant> {
    (ms >= 0).then(|| Instant::now() + Duration::from_millis(ms as u64))
}
fn remaining(until: Option<Instant>) -> i64 {
    until
        .map(|until| {
            until
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(i32::MAX as u128) as i64
        })
        .unwrap_or(-1)
}
fn wait(fd: RawFd, events: i16, ms: i64) -> io::Result<bool> {
    let until = deadline(ms);
    loop {
        let mut descriptor = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut descriptor, 1, remaining(until) as i32) };
        if result > 0 {
            if descriptor.revents & libc::POLLNVAL != 0 {
                return Err(io::Error::from_raw_os_error(libc::EBADF));
            }
            return Ok(true); // HUP/ERR go through read/write/accept, preserving EOF/OS errors.
        }
        if result == 0 {
            return Ok(false);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}
unsafe extern "C" fn drop_handle(data: *mut c_void) {
    let _ = std::panic::catch_unwind(|| drop(unsafe { Box::from_raw(data.cast::<Handle>()) }));
}
fn object(call: &Call<'_>, resource: Resource) -> Result<Value, String> {
    let methods = if matches!(resource, Resource::Listener(_)) {
        r#"{"accept":"__stdAccept","address":"__stdAddress","close":"__stdClose"}"#
    } else {
        r#"{"read":"__stdRead","readUntil":"__stdReadUntil","readExactly":"__stdReadExactly","writeAll":"__stdWriteAll","address":"__stdAddress","close":"__stdClose"}"#
    };
    let data = Box::into_raw(Box::new(Handle {
        resource: Some(resource),
        buffer: Vec::new(),
    }));
    match unsafe { call.object_new(KIND, data.cast(), drop_handle, methods) } {
        Ok(value) => Ok(value),
        Err(error) => {
            drop(unsafe { Box::from_raw(data) });
            Err(error)
        }
    }
}
// SAFETY: Call is ABI metadata, not the payload. The caller must exclusively borrow the
// VM-owned payload for this serialized invocation; no reference may escape the call.
#[allow(clippy::mut_from_ref)]
unsafe fn handle<'a>(call: &'a Call<'_>) -> Result<&'a mut Handle, String> {
    let data = unsafe { call.object_data(call.argument(0)?, KIND)? };
    Ok(unsafe { data.cast::<Handle>().as_mut() })
}
#[allow(clippy::mut_from_ref)]
unsafe fn resource<'a>(call: &'a Call<'_>) -> Result<&'a mut Resource, String> {
    unsafe { handle(call)? }
        .resource
        .as_mut()
        .ok_or_else(|| "Stream is closed".into())
}
#[allow(clippy::mut_from_ref)]
unsafe fn readable_handle<'a>(call: &'a Call<'_>) -> Result<&'a mut Handle, String> {
    let handle = unsafe { handle(call)? };
    match handle.resource.as_ref() {
        Some(Resource::Socket(_) | Resource::Input(_)) => Ok(handle),
        None => Err("Stream is closed".into()),
        _ => Err("Stream is not readable".into()),
    }
}
fn binary_api() -> Result<&'static BytesV1, String> {
    BYTES
        .get()
        .copied()
        .ok_or_else(|| "Missing binary capability".into())
}
fn socket(call: &Call<'_>, stream: TcpStream) -> Result<Value, String> {
    stream
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    stream
        .set_nodelay(true)
        .map_err(|error| error.to_string())?;
    object(call, Resource::Socket(stream))
}
fn stdio(call: &Call<'_>, fd: RawFd, input: bool) -> Result<Value, String> {
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let file = unsafe { File::from_raw_fd(duplicate) };
    object(
        call,
        if input {
            Resource::Input(file)
        } else {
            Resource::Output(file)
        },
    )
}
macro_rules! callback {
    ($name:ident, $call:ident, $body:block) => {
        unsafe extern "C" fn $name(
            ctx: Context,
            api: *const ValuesV1,
            _: *mut c_void,
            args: *const Value,
            len: usize,
            out: *mut Value,
        ) -> i32 {
            unsafe {
                guarded(ctx, api, out, || {
                    let $call = Call::new(ctx, api, args, len);
                    $body
                })
            }
        }
    };
}
callback!(listen, call, {
    let port = integer(&call, 0, 0, 65535)? as u16;
    let host = call.string(call.argument(1)?)?;
    let listener = TcpListener::bind((host.as_str(), port)).map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    object(&call, Resource::Listener(listener))
});
callback!(connect, call, {
    let host = call.string(call.argument(0)?)?;
    let port = integer(&call, 1, 1, 65535)? as u16;
    let ms = integer(&call, 2, 1, i32::MAX as i64)?;
    let addresses = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?;
    let until = deadline(ms);
    let mut error = "No TCP addresses resolved".to_string();
    for address in addresses {
        let left = remaining(until);
        if left == 0 {
            return Err("TCP connect timed out".into());
        }
        match TcpStream::connect_timeout(&address, Duration::from_millis(left as u64)) {
            Ok(stream) => return socket(&call, stream),
            Err(cause) => error = cause.to_string(),
        }
    }
    Err(error)
});
callback!(input, call, { stdio(&call, libc::STDIN_FILENO, true) });
callback!(output, call, { stdio(&call, libc::STDOUT_FILENO, false) });
callback!(accept, call, {
    let ms = timeout(&call, 1)?;
    let Resource::Listener(listener) = resource(&call)? else {
        return Err("Expected TCP listener".into());
    };
    let until = deadline(ms);
    loop {
        if ms > 0 && until.is_some_and(|until| Instant::now() >= until) {
            return call.null_new();
        }
        if !wait(listener.as_raw_fd(), libc::POLLIN, remaining(until))
            .map_err(|error| error.to_string())?
        {
            return call.null_new();
        }
        match listener.accept() {
            Ok((stream, _)) => return socket(&call, stream),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
});
callback!(read, call, {
    let size = integer(&call, 1, 1, MAX_READ as i64)? as usize;
    let ms = timeout(&call, 2)?;
    let handle = readable_handle(&call)?;
    if handle.buffer.is_empty() {
        match handle.fill(deadline(ms), ms > 0)? {
            Fill::Timeout => return call.null_new(),
            Fill::Eof => return call.bytes_new(binary_api()?, &[]),
            Fill::Data => (),
        }
    }
    let count = size.min(handle.buffer.len());
    call.bytes_new(binary_api()?, &handle.take(count))
});
callback!(read_until, call, {
    let separator = call.bytes(binary_api()?, call.argument(1)?)?;
    let max = integer(&call, 2, 1, MAX_READ as i64)? as usize;
    if separator.is_empty() || separator.len() > max {
        return Err("Separator must be nonempty and fit in maxBytes".into());
    }
    let ms = timeout(&call, 3)?;
    let until = deadline(ms);
    let handle = readable_handle(&call)?;
    loop {
        // ponytail: rescans a bounded prefix; use incremental delimiter search for large lines.
        if let Some(index) = handle
            .buffer
            .windows(separator.len())
            .position(|part| part == separator)
        {
            let count = index + separator.len();
            if count > max {
                return Err("Stream delimiter exceeds maxBytes".into());
            }
            return call.bytes_new(binary_api()?, &handle.take(count));
        }
        if handle.buffer.len() >= max {
            return Err("Stream delimiter exceeds maxBytes".into());
        }
        match handle.fill(until, ms > 0)? {
            Fill::Timeout => return call.null_new(),
            Fill::Eof if handle.buffer.is_empty() => return call.bytes_new(binary_api()?, &[]),
            Fill::Eof => return Err("EOF before stream delimiter".into()),
            Fill::Data => (),
        }
    }
});
callback!(read_exactly, call, {
    let size = integer(&call, 1, 0, 64 * 1024 * 1024)? as usize;
    let ms = timeout(&call, 2)?;
    let until = deadline(ms);
    let handle = readable_handle(&call)?;
    while handle.buffer.len() < size {
        match handle.fill(until, ms > 0)? {
            Fill::Timeout => return call.null_new(),
            Fill::Eof => {
                return Err(format!(
                    "EOF before {size} stream bytes (received {})",
                    handle.buffer.len()
                ));
            }
            Fill::Data => (),
        }
    }
    call.bytes_new(binary_api()?, &handle.take(size))
});
callback!(utf8_decode, call, {
    let text = String::from_utf8(call.bytes(binary_api()?, call.argument(0)?)?)
        .map_err(|error| error.to_string())?;
    call.string_new(&text)
});
callback!(write_all, call, {
    let bytes = call.bytes(binary_api()?, call.argument(1)?)?;
    let ms = timeout(&call, 2)?;
    let resource = resource(&call)?;
    if !matches!(resource, Resource::Socket(_) | Resource::Output(_)) {
        return Err("Stream is not writable".into());
    }
    let until = deadline(ms);
    let mut offset = 0;
    while offset < bytes.len() {
        if ms > 0 && until.is_some_and(|until| Instant::now() >= until) {
            return Err(format!("Stream write timed out after {offset} bytes"));
        }
        if !wait(resource.fd(), libc::POLLOUT, remaining(until))
            .map_err(|error| error.to_string())?
        {
            return Err(format!("Stream write timed out after {offset} bytes"));
        }
        // Pipe writes are <= PIPE_BUF; sockets are nonblocking. Never silently drop a short write.
        let part = &bytes[offset..(offset + 4096).min(bytes.len())];
        let result = match resource {
            Resource::Socket(stream) => stream.write(part),
            Resource::Output(file) => file.write(part),
            _ => unreachable!(),
        };
        match result {
            Ok(0) => return Err("Stream write returned zero bytes".into()),
            Ok(count) => offset += count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(error) => return Err(format!("Stream write failed after {offset} bytes: {error}")),
        }
    }
    call.number_new(offset as f64)
});
callback!(close, call, {
    let handle = handle(&call)?;
    handle.resource.take();
    handle.buffer.clear();
    call.null_new()
});
callback!(address, call, {
    let address = match resource(&call)? {
        Resource::Listener(listener) => listener.local_addr(),
        Resource::Socket(stream) => stream.local_addr(),
        _ => return Err("Standard streams have no TCP address".into()),
    }
    .map_err(|error| error.to_string())?;
    let value = call.struct_new()?;
    call.struct_set(value, "host", call.string_new(&address.ip().to_string())?)?;
    call.struct_set(value, "port", call.number_new(address.port() as f64)?)?;
    Ok(value)
});

/// # Safety
/// The host must supply its live ABI-1 registration pointers for this initialization.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn matchbox_plugin_init_v1(host: *const HostV1, ctx: Context) -> i32 {
    let result = std::panic::catch_unwind(|| unsafe {
        if let Err(status) = require_values(host, ctx) {
            return status;
        }
        let bytes = match require_bytes(host, ctx) {
            Ok(api) => api,
            Err(status) => return status,
        };
        if let Some(previous) = BYTES.get() {
            if !std::ptr::eq(*previous, bytes) {
                ((*host).error)(ctx, Text::new("Binary capability belongs to another host"));
                return 1;
            }
        } else if BYTES.set(bytes).is_err() {
            return 1;
        }
        let registrations: [(&str, Invoke); 12] = [
            (
                r#"{"name":"__stdTcpListen","arguments":[{"name":"port","default":0},{"name":"host","default":"127.0.0.1"}]}"#,
                listen,
            ),
            (
                r#"{"name":"__stdTcpConnect","arguments":[{"name":"host","required":true},{"name":"port","required":true},{"name":"timeoutMs","default":5000}]}"#,
                connect,
            ),
            (r#"{"name":"__stdInput"}"#, input),
            (r#"{"name":"__stdOutput"}"#, output),
            (
                r#"{"name":"__stdAccept","arguments":[{"name":"listener","required":true},{"name":"timeoutMs","default":-1}]}"#,
                accept,
            ),
            (
                r#"{"name":"__stdRead","arguments":[{"name":"stream","required":true},{"name":"maxBytes","default":8192},{"name":"timeoutMs","default":-1}]}"#,
                read,
            ),
            (
                r#"{"name":"__stdReadUntil","arguments":[{"name":"stream","required":true},{"name":"separator","required":true},{"name":"maxBytes","default":8192},{"name":"timeoutMs","default":-1}]}"#,
                read_until,
            ),
            (
                r#"{"name":"__stdReadExactly","arguments":[{"name":"stream","required":true},{"name":"size","required":true},{"name":"timeoutMs","default":-1}]}"#,
                read_exactly,
            ),
            (
                r#"{"name":"__stdUtf8Decode","arguments":[{"name":"data","required":true}]}"#,
                utf8_decode,
            ),
            (
                r#"{"name":"__stdWriteAll","arguments":[{"name":"stream","required":true},{"name":"data","required":true},{"name":"timeoutMs","default":5000}]}"#,
                write_all,
            ),
            (
                r#"{"name":"__stdClose","arguments":[{"name":"stream","required":true}]}"#,
                close,
            ),
            (
                r#"{"name":"__stdAddress","arguments":[{"name":"stream","required":true}]}"#,
                address,
            ),
        ];
        for (metadata, invoke) in registrations {
            let status = ((*host).register)(ctx, Text::new(metadata), invoke);
            if status != 0 {
                return status;
            }
        }
        0
    });
    result.unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn poll_distinguishes_timeout_data_and_eof() {
        let (mut reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
        assert!(!wait(reader.as_raw_fd(), libc::POLLIN, 5).unwrap());
        writer.write_all(&[0, 255, 13, 10]).unwrap();
        assert!(wait(reader.as_raw_fd(), libc::POLLIN, 50).unwrap());
        let mut data = [0; 4];
        reader.read_exact(&mut data).unwrap();
        assert_eq!(data, [0, 255, 13, 10]);
        drop(writer);
        assert!(wait(reader.as_raw_fd(), libc::POLLIN, 50).unwrap());
        assert_eq!(reader.read(&mut data).unwrap(), 0);
    }
}
