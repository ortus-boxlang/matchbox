use crate::types::{BxVM, BxValue};

#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
use regex::Regex;
#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
use std::fs::File;
#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
use std::io::{Read, Write};
#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
use std::process::{Command, Stdio};
#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
use std::sync::{Arc, Mutex};
#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
use std::thread;
#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
use std::time::{Duration, Instant};

/// Executes a command with optional bounded stdin, output redirection and Unix group ownership.
///
/// The native extension at position 12, `terminateProcessGroup`, requires `terminateOnTimeout`.
/// Owned groups are killed on timeout, normal completion, error and orderly native host shutdown.
///
/// # Errors
/// Returns argument, spawn, I/O or wait failures. Group ownership is Unix-only.
#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
pub fn system_execute(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("systemExecute() expects at least a process name".to_string());
    }

    let executable = vm.to_string(args[0]);
    if executable.is_empty() {
        return Err("systemExecute() process name must not be empty".to_string());
    }

    let mut command = Command::new(&executable);
    if let Some(arguments) = args.get(1) {
        if !arguments.is_null() {
            if vm.is_array_value(*arguments) {
                let arguments_id = arguments.as_gc_id().unwrap();
                for index in 0..vm.array_len(arguments_id) {
                    command.arg(vm.to_string(vm.array_get(arguments_id, index)));
                }
            } else if vm.is_string_value(*arguments) || arguments.is_number() || arguments.is_bool()
            {
                let argument_string = vm.to_string(*arguments);
                let argument_pattern = Regex::new(r#"[^\s\"']+|\"[^\"]*\"|'[^']*'"#)
                    .map_err(|error| error.to_string())?;
                for argument in argument_pattern.find_iter(&argument_string) {
                    let argument = argument.as_str();
                    let unquoted = if argument.len() >= 2
                        && ((argument.starts_with('"') && argument.ends_with('"'))
                            || (argument.starts_with('\'') && argument.ends_with('\'')))
                    {
                        &argument[1..argument.len() - 1]
                    } else {
                        argument
                    };
                    command.arg(unquoted);
                }
            } else {
                return Err("systemExecute() arguments must be a string or an array".to_string());
            }
        }
    }

    let timeout_seconds = coerce_non_negative_long(vm, args.get(2), "timeout")?;
    let terminate_on_timeout = coerce_bool(vm, args.get(3), false, "terminateOnTimeout")?;

    if let Some(directory) = args.get(4).filter(|value| !value.is_null()) {
        command.current_dir(vm.to_string(*directory));
    }

    let output_path = args
        .get(5)
        .filter(|value| !value.is_null())
        .map(|value| vm.to_string(*value))
        .filter(|value| !value.is_empty());
    let error_path = args
        .get(6)
        .filter(|value| !value.is_null())
        .map(|value| vm.to_string(*value))
        .filter(|value| !value.is_empty());

    let inherit_environment = coerce_bool(vm, args.get(7), true, "inheritEnvironment")?;
    if !inherit_environment {
        command.env_clear();
    }
    if let Some(environment) = args.get(8).filter(|value| !value.is_null()) {
        let environment_id = environment
            .as_gc_id()
            .filter(|_| vm.is_struct_value(*environment))
            .ok_or_else(|| "systemExecute() environment must be a struct".to_string())?;
        for key in vm.struct_key_array(environment_id) {
            command.env(&key, vm.to_string(vm.struct_get(environment_id, &key)));
        }
    }
    let inherit_io = coerce_bool(vm, args.get(9), false, "inheritIO")?;
    if inherit_io && (output_path.is_some() || error_path.is_some()) {
        return Err(
            "systemExecute() inheritIO cannot be combined with output or error files".to_string(),
        );
    }
    let input = args
        .get(10)
        .filter(|value| !value.is_null())
        .map(|value| vm.to_string(*value));
    if inherit_io && input.is_some() {
        return Err("systemExecute() inheritIO cannot be combined with input".to_string());
    }

    let terminate_group = coerce_bool(vm, args.get(11), false, "terminateProcessGroup")?;
    if terminate_group && !terminate_on_timeout {
        return Err(
            "systemExecute() terminateProcessGroup requires terminateOnTimeout=true".into(),
        );
    }

    if inherit_io {
        command.stdin(Stdio::inherit());
        command.stdout(Stdio::inherit());
        command.stderr(Stdio::inherit());
    } else if input.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    if !inherit_io {
        if let Some(path) = &output_path {
            command.stdout(Stdio::from(File::create(path).map_err(|error| {
                format!("systemExecute() could not open output file '{path}': {error}")
            })?));
        } else {
            command.stdout(Stdio::piped());
        }
        if let Some(path) = &error_path {
            command.stderr(Stdio::from(File::create(path).map_err(|error| {
                format!("systemExecute() could not open error file '{path}': {error}")
            })?));
        } else {
            command.stderr(Stdio::piped());
        }
    }

    let (mut child, group) =
        crate::process_groups::ProcessGroup::spawn(&mut command, terminate_group).map_err(
            |error| format!("systemExecute() could not execute '{executable}': {error}"),
        )?;
    // Drain output before writing stdin: a child can write before it reads. A blocked stdin
    // write must not prevent timeout or orderly host shutdown from stopping the child.
    let input_thread = input.and_then(|input| {
        child
            .stdin
            .take()
            .map(|mut stdin| thread::spawn(move || stdin.write_all(input.as_bytes())))
    });
    let pid = child.id();
    let stdout_thread = child.stdout.take().map(spawn_stream_reader);
    let stderr_thread = child.stderr.take().map(spawn_stream_reader);

    let deadline = if timeout_seconds > 0 {
        Some(Instant::now() + Duration::from_secs(timeout_seconds as u64))
    } else {
        None
    };
    let mut timed_out = false;
    let mut terminated = false;
    let mut exit_code = None;

    loop {
        let status = match &group {
            Some(group) => group.try_wait(&mut child),
            None => child.try_wait().map_err(|e| e.to_string()),
        }
        .map_err(|error| format!("systemExecute() failed waiting for process: {error}"))?;
        if let Some(status) = status {
            exit_code = status.code();
            break;
        }

        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            timed_out = true;
            if terminate_on_timeout {
                let killed = match &group {
                    Some(group) => group.terminate(),
                    None => child.kill(),
                };
                if let Err(error) = killed {
                    if group.is_some() {
                        return Err(format!(
                            "systemExecute() failed terminating process group: {error}"
                        ));
                    }
                    if child
                        .try_wait()
                        .map_err(|wait_error| {
                            format!("systemExecute() failed waiting for process: {wait_error}")
                        })?
                        .is_none()
                    {
                        return Err(format!(
                            "systemExecute() failed terminating process: {error}"
                        ));
                    }
                }
                let status = child.wait().map_err(|error| {
                    format!("systemExecute() failed waiting for terminated process: {error}")
                })?;
                exit_code = status.code();
                terminated = true;
            } else {
                thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            break;
        }

        thread::sleep(Duration::from_millis(10));
    }

    if !timed_out || terminated {
        wait_for_streams(&stdout_thread, &stderr_thread, Duration::from_secs(5));
    }
    if let Some(writer) = input_thread.filter(|writer| writer.is_finished()) {
        let written = writer
            .join()
            .map_err(|_| "systemExecute() input writer failed".to_string())?;
        // Closing stdin is expected when a deadline/shutdown kills the reader. Do not
        // replace the timeout/exit outcome with a scheduling-dependent broken-pipe error.
        if !timed_out && exit_code == Some(0) {
            written.map_err(|e| format!("systemExecute() failed writing process input: {e}"))?;
        }
    }
    let stdout = captured_output(&stdout_thread);
    let stderr = captured_output(&stderr_thread);

    let result_id = vm.struct_new();
    set_optional_string(
        vm,
        result_id,
        "output",
        stdout,
        output_path.is_some() || inherit_io || terminated,
    );
    set_optional_string(
        vm,
        result_id,
        "error",
        stderr,
        error_path.is_some() || inherit_io || terminated,
    );
    vm.struct_set(result_id, "timeout", BxValue::new_bool(timed_out));
    vm.struct_set(result_id, "terminated", BxValue::new_bool(terminated));
    vm.struct_set(result_id, "pid", BxValue::new_number(pid as f64));
    if let Some(exit_code) = exit_code {
        vm.struct_set(result_id, "exitCode", BxValue::new_number(exit_code as f64));
    }
    Ok(BxValue::new_ptr(result_id))
}

#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
type StreamReader = (thread::JoinHandle<()>, Arc<Mutex<Vec<u8>>>);

#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
fn spawn_stream_reader(mut stream: impl Read + Send + 'static) -> StreamReader {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let thread_bytes = Arc::clone(&bytes);
    let handle = thread::spawn(move || {
        let mut chunk = [0_u8; 8192];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if let Ok(mut output) = thread_bytes.lock() {
                        output.extend_from_slice(&chunk[..count]);
                    }
                }
            }
        }
    });
    (handle, bytes)
}

#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
fn wait_for_streams(stdout: &Option<StreamReader>, stderr: &Option<StreamReader>, grace: Duration) {
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        let stdout_finished = stdout.as_ref().is_none_or(|reader| reader.0.is_finished());
        let stderr_finished = stderr.as_ref().is_none_or(|reader| reader.0.is_finished());
        if stdout_finished && stderr_finished {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
fn captured_output(reader: &Option<StreamReader>) -> Option<String> {
    reader.as_ref().map(|(_, bytes)| {
        let bytes = bytes.lock().map(|value| value.clone()).unwrap_or_default();
        normalize_output(&String::from_utf8_lossy(&bytes))
    })
}

#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
fn normalize_output(value: &str) -> String {
    value.lines().collect::<Vec<_>>().join("\n")
}

#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
fn coerce_non_negative_long(
    vm: &dyn BxVM,
    value: Option<&BxValue>,
    name: &str,
) -> Result<i64, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(0);
    };
    let number = if value.is_number() {
        value.as_number()
    } else if value.is_bool() {
        if value.as_bool() { 1.0 } else { 0.0 }
    } else if vm.is_string_value(*value) {
        vm.to_string(*value)
            .parse::<f64>()
            .map_err(|_| format!("systemExecute() {name} must be an integer"))?
    } else {
        return Err(format!("systemExecute() {name} must be an integer"));
    };
    if !number.is_finite() || number < 0.0 || number > i64::MAX as f64 {
        return Err(format!(
            "systemExecute() {name} must be a non-negative integer"
        ));
    }
    Ok(number as i64)
}

#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
fn coerce_bool(
    vm: &dyn BxVM,
    value: Option<&BxValue>,
    default: bool,
    name: &str,
) -> Result<bool, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(default);
    };
    if value.is_bool() {
        return Ok(value.as_bool());
    }
    if value.is_number() {
        return Ok(value.as_number() != 0.0);
    }
    if vm.is_string_value(*value) {
        return match vm.to_string(*value).to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(true),
            "false" | "no" | "0" | "" => Ok(false),
            _ => Err(format!("systemExecute() {name} must be a boolean")),
        };
    }
    Err(format!("systemExecute() {name} must be a boolean"))
}

#[cfg(not(any(target_family = "wasm", target_os = "espidf")))]
fn set_optional_string(
    vm: &mut dyn BxVM,
    result_id: usize,
    key: &str,
    value: Option<String>,
    redirected: bool,
) {
    if redirected {
        vm.struct_set(result_id, key, BxValue::new_null());
    } else {
        let value_id = vm.string_new(value.unwrap_or_default());
        vm.struct_set(result_id, key, BxValue::new_ptr(value_id));
    }
}

/// Rejects command execution on targets without native process support.
///
/// # Errors
/// Always returns an unsupported-target error.
#[cfg(any(target_family = "wasm", target_os = "espidf"))]
pub fn system_execute(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Err("systemExecute() is not supported on this target".to_string())
}
