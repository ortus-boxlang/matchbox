use esp32_nimble::{
    utilities::{mutex::Mutex, BleUuid},
    BLEAddressType,
    BLEAddress, BLEAdvertisedDevice, BLEClient, BLEDevice, BLERemoteCharacteristic,
    BLERemoteService, BLEScan,
};
use esp_idf_svc::hal::task::block_on;
use std::ffi::CString;
use std::sync::mpsc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct PrintResult {
    pub device_name: String,
    pub device_id: String,
    pub write_mode: String,
    pub payload_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct PrinterConnectionInfo {
    pub handle_id: u32,
    pub device_name: String,
    pub device_id: String,
    pub write_mode: String,
    pub characteristic_uuid: String,
}

#[derive(Clone, Debug, Default)]
struct AdapterHandle;

#[derive(Clone, Debug)]
struct DeviceHandle {
    address: BLEAddress,
    id: String,
    name: String,
}

#[derive(Clone)]
struct ConnectionHandle {
    client: Arc<Mutex<BLEClient>>,
}

impl std::fmt::Debug for ConnectionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionHandle").finish()
    }
}

#[derive(Clone, Debug)]
struct CharacteristicHandle {
    inner: BLERemoteCharacteristic,
    uuid: String,
    write: bool,
    write_without_response: bool,
}

#[derive(Clone, Debug)]
struct ServiceHandle {
    characteristics: Vec<CharacteristicHandle>,
}

#[derive(Clone)]
struct ActivePrinterConnection {
    connection: ConnectionHandle,
    characteristic: CharacteristicHandle,
    write_mode: WriteMode,
    device_name: String,
    device_id: String,
}

// Embedded-only connection store wrapper. `esp32-nimble` remote connection
// types are not `Sync`, but the runner only accesses them through a serialized
// mutex path. Keep this isolated here until the embedded runtime owns a cleaner
// resource model.
struct ActivePrinterConnectionStore {
    ptr: usize,
}

impl ActivePrinterConnectionStore {
    fn new() -> Self {
        let map = Box::new(std::collections::HashMap::<u32, ActivePrinterConnection>::new());
        Self {
            ptr: Box::into_raw(map) as usize,
        }
    }

    fn with_map<R>(
        &mut self,
        f: impl FnOnce(&mut std::collections::HashMap<u32, ActivePrinterConnection>) -> R,
    ) -> R {
        let map = unsafe {
            &mut *(self.ptr as *mut std::collections::HashMap<u32, ActivePrinterConnection>)
        };
        f(map)
    }
}

unsafe impl Send for ActivePrinterConnectionStore {}
unsafe impl Sync for ActivePrinterConnectionStore {}

#[derive(Clone, Debug, Default)]
struct ScanOptions {
    timeout_ms: u64,
    services: Vec<Uuid>,
    name_prefix: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WriteMode {
    WithResponse,
    WithoutResponse,
}

struct WorkerTask<T> {
    job: Option<Box<dyn FnOnce() -> Result<T, String> + Send>>,
    tx: mpsc::SyncSender<Result<T, String>>,
}

fn run_on_worker_task<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    let task = Box::new(WorkerTask {
        job: Some(Box::new(job)),
        tx,
    });

    extern "C" fn task_entry<T: Send + 'static>(param: *mut std::ffi::c_void) {
        let mut task = unsafe { Box::from_raw(param.cast::<WorkerTask<T>>()) };
        let result = task.job.take().expect("worker task job missing")();
        let _ = task.tx.send(result);
        unsafe { esp_idf_svc::sys::vTaskDelete(std::ptr::null_mut()) };
    }

    let name = CString::new("mb_ble_worker").map_err(|error| error.to_string())?;
    let res = unsafe {
        esp_idf_svc::sys::xTaskCreatePinnedToCore(
            Some(task_entry::<T>),
            name.as_ptr(),
            16 * 1024,
            Box::into_raw(task).cast(),
            5,
            std::ptr::null_mut(),
            0,
        )
    };
    if res != 1 {
        return Err(format!("failed to create BLE worker task ({})", res));
    }
    rx.recv().map_err(|error| error.to_string())?
}

fn init_ble_runtime() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        esp_idf_svc::sys::link_patches();
    });
}

fn ble_device() -> &'static BLEDevice {
    init_ble_runtime();
    BLEDevice::take()
}

pub fn ensure_ble_ready() -> Result<(), String> {
    let _ = ble_device();
    println!("[matchbox] BLE runtime initialized");
    Ok(())
}

pub fn shutdown_ble() -> Result<(), String> {
    BLEDevice::deinit_full().map_err(|error| error.to_string())?;
    *cached_printer().lock().unwrap() = None;
    active_printer_connections()
        .lock()
        .unwrap()
        .with_map(|connections| connections.clear());
    println!("[matchbox] BLE runtime shutdown");
    Ok(())
}

fn cached_printer() -> &'static StdMutex<Option<DeviceHandle>> {
    static CACHE: OnceLock<StdMutex<Option<DeviceHandle>>> = OnceLock::new();
    CACHE.get_or_init(|| StdMutex::new(None))
}

fn active_printer_connections(
) -> &'static StdMutex<ActivePrinterConnectionStore> {
    static CONNECTIONS: OnceLock<StdMutex<ActivePrinterConnectionStore>> = OnceLock::new();
    CONNECTIONS.get_or_init(|| StdMutex::new(ActivePrinterConnectionStore::new()))
}

fn next_connection_id() -> u32 {
    static NEXT_ID: AtomicU32 = AtomicU32::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

fn address_type_label(addr_type: &BLEAddressType) -> &'static str {
    match addr_type {
        BLEAddressType::Public => "public",
        BLEAddressType::Random => "random",
        BLEAddressType::PublicID => "public-id",
        BLEAddressType::RandomID => "random-id",
    }
}

fn parse_address_type(input: &str) -> Option<BLEAddressType> {
    match input.trim().to_ascii_lowercase().as_str() {
        "public" => Some(BLEAddressType::Public),
        "random" => Some(BLEAddressType::Random),
        "public-id" | "publicid" => Some(BLEAddressType::PublicID),
        "random-id" | "randomid" => Some(BLEAddressType::RandomID),
        _ => None,
    }
}

fn normalize_ble_uuid(uuid: BleUuid) -> String {
    uuid.to_string().to_lowercase()
}

fn service_filter_matches(_device: &BLEAdvertisedDevice, _filters: &[Uuid]) -> bool {
    true
}

fn characteristic_flags(characteristic: &BLERemoteCharacteristic) -> (bool, bool) {
    let _ = characteristic;
    (true, true)
}

fn select_characteristic(
    services: &[ServiceHandle],
    preferred_characteristic_uuid: &str,
) -> Result<(CharacteristicHandle, WriteMode), String> {
    let preferred = preferred_characteristic_uuid.to_ascii_lowercase();
    let mut selected: Option<(CharacteristicHandle, WriteMode)> = None;

    for service in services {
        for characteristic in &service.characteristics {
            if !(characteristic.write || characteristic.write_without_response) {
                continue;
            }

            let mode = if characteristic.write_without_response {
                WriteMode::WithoutResponse
            } else {
                WriteMode::WithResponse
            };

            if selected.is_none() {
                selected = Some((characteristic.clone(), mode));
            }

            if characteristic.uuid == preferred {
                selected = Some((characteristic.clone(), mode));
                break;
            }
        }

        if let Some((ref characteristic, _)) = selected {
            if characteristic.uuid == preferred {
                break;
            }
        }
    }

    selected.ok_or_else(|| "No writable BLE characteristic was discovered.".to_string())
}

fn get_default_adapter() -> Result<AdapterHandle, String> {
    init_ble_runtime();
    Ok(AdapterHandle)
}

fn scan(_adapter: &AdapterHandle, options: &ScanOptions) -> Result<Vec<DeviceHandle>, String> {
    let timeout_ms = options.timeout_ms.max(1);
    let name_prefix = options.name_prefix.clone();
    let service_filters = options.services.clone();
    println!(
        "[esp32-printer] scan start timeoutMs={} namePrefix={:?}",
        timeout_ms, name_prefix
    );
    run_on_worker_task(move || {
        let max_attempts = 3;

        for attempt in 1..=max_attempts {
            let found = Arc::new(Mutex::new(Vec::<DeviceHandle>::new()));
            let found_ref = Arc::clone(&found);
            let service_filters_for_attempt = service_filters.clone();

            println!("[esp32-printer] scan attempt {}", attempt);
            let scan_result: Result<Option<()>, String> = block_on(async {
                let mut ble_scan = BLEScan::new();
                ble_scan
                    .active_scan(true)
                    .filter_duplicates(false)
                    .interval(160)
                    .window(159)
                    .start(ble_device(), timeout_ms as _, move |device, data| {
                        let name = data.name().map(|name| name.to_string()).unwrap_or_default();
                        let id = format!("{:?}", device.addr());
                        println!(
                            "[esp32-printer] discovered id={} name={:?}",
                            id, name
                        );

                        if !service_filters_for_attempt.is_empty()
                            && !service_filter_matches(device, &service_filters_for_attempt)
                        {
                            return None::<()>;
                        }

                        let mut devices = found_ref.lock();
                        if let Some(existing) = devices.iter_mut().find(|existing| existing.id == id)
                        {
                            if existing.name.is_empty() && !name.is_empty() {
                                existing.name = name;
                            }
                            return None::<()>;
                        }

                        devices.push(DeviceHandle {
                            address: device.addr(),
                            id,
                            name,
                        });
                        None::<()>
                    })
                    .await
                    .map_err(|error| error.to_string())
            });
            let _ = scan_result?;

            let mut devices = found.lock().clone();
            if let Some(prefix) = &name_prefix {
                let prefix_lower = prefix.to_ascii_lowercase();
                devices
                    .retain(|device| device.name.to_ascii_lowercase().starts_with(&prefix_lower));
            }

            if !devices.is_empty() {
                println!("[esp32-printer] scan complete devices={:?}", devices);
                return Ok(devices);
            }

            println!("[esp32-printer] scan attempt {} found no matching devices", attempt);
        }

        println!("[esp32-printer] scan complete devices=[]");
        Ok(Vec::new())
    })
}

fn connect(device: &DeviceHandle) -> Result<ConnectionHandle, String> {
    let address = device.address;
    block_on(async move {
        let client = Arc::new(Mutex::new(ble_device().new_client()));
        {
            let mut locked = client.lock();
            locked.connect(&address).await.map_err(|error| error.to_string())?;
        }
        Ok(ConnectionHandle { client })
    })
}

fn disconnect(connection: &ConnectionHandle) -> Result<(), String> {
    let mut client = connection.client.lock();
    client.disconnect().map_err(|error| error.to_string())
}

fn discover_services(connection: &ConnectionHandle) -> Result<Vec<ServiceHandle>, String> {
    block_on(async {
        let mut client = connection.client.lock();
        let services = client
            .get_services()
            .await
            .map_err(|error| error.to_string())?;

        let mut out = Vec::new();
        for service in services {
            let mut characteristics_out = Vec::new();
            let characteristics = service
                .get_characteristics()
                .await
                .map_err(|error| error.to_string())?;

            for characteristic in characteristics {
                let (write, write_without_response) = characteristic_flags(characteristic);
                characteristics_out.push(CharacteristicHandle {
                    uuid: normalize_ble_uuid(characteristic.uuid()),
                    write,
                    write_without_response,
                    inner: characteristic.clone(),
                });
            }

            let _service_uuid = normalize_ble_uuid(service.uuid());
            out.push(ServiceHandle {
                characteristics: characteristics_out,
            });
        }

        Ok(out)
    })
}

fn write(
    characteristic: &CharacteristicHandle,
    data: &[u8],
    mode: WriteMode,
) -> Result<(), String> {
    // Avoid NimBLE's long-write path on constrained targets. These printers
    // accept streamed chunks well, and chunking keeps us under the ATT payload
    // size that would otherwise trigger a larger, more fragile transaction.
    const CHUNK_SIZE: usize = 60;
    const WITHOUT_RESPONSE_DELAY_TICKS: u32 = 8;

    let mut characteristic = characteristic.inner.clone();
    for chunk in data.chunks(CHUNK_SIZE) {
        block_on(async {
            characteristic
                .write_value(chunk, matches!(mode, WriteMode::WithResponse))
                .await
                .map_err(|error| error.to_string())
        })?;

        if matches!(mode, WriteMode::WithoutResponse) {
            unsafe { esp_idf_svc::sys::vTaskDelay(WITHOUT_RESPONSE_DELAY_TICKS) };
        }
    }

    Ok(())
}

pub fn print_bytes(
    name_prefix: &str,
    preferred_characteristic_uuid: &str,
    timeout_ms: u64,
    payload: &[u8],
) -> Result<PrintResult, String> {
    if let Some(device) = cached_printer().lock().unwrap().clone() {
        println!(
            "[esp32-printer] trying cached printer id={} name={:?}",
            device.id, device.name
        );
        match try_print_to_device(&device, preferred_characteristic_uuid, payload) {
            Ok(print) => return Ok(print),
            Err(error) => {
                println!("[esp32-printer] cached printer failed: {}", error);
            }
        }
    }

    let adapter = get_default_adapter()?;
    let devices = scan(
        &adapter,
        &ScanOptions {
            timeout_ms,
            services: Vec::new(),
            name_prefix: Some(name_prefix.to_string()),
        },
    )?;

    let device = devices
        .into_iter()
        .next()
        .ok_or_else(|| format!("No BLE printer was found with prefix '{}'", name_prefix))?;

    let print = try_print_to_device(&device, preferred_characteristic_uuid, payload)?;
    *cached_printer().lock().unwrap() = Some(device);
    Ok(print)
}

pub fn print_bytes_to_address(
    address: &str,
    preferred_characteristic_uuid: &str,
    payload: &[u8],
    preferred_address_type: Option<&str>,
) -> Result<PrintResult, String> {
    let mut last_error: Option<String> = None;
    let mut addr_types = Vec::new();

    if let Some(input) = preferred_address_type {
        if let Some(parsed) = parse_address_type(input) {
            addr_types.push(parsed);
        } else {
            return Err(format!("Unsupported BLE address type '{}'", input));
        }
    }

    for fallback in [
        BLEAddressType::Random,
        BLEAddressType::Public,
        BLEAddressType::RandomID,
        BLEAddressType::PublicID,
    ] {
        if !addr_types.iter().any(|existing| address_type_label(existing) == address_type_label(&fallback)) {
            addr_types.push(fallback);
        }
    }

    for addr_type in addr_types {
        let addr_type_label = address_type_label(&addr_type);
        let Some(parsed) = BLEAddress::from_str(address, addr_type) else {
            return Err(format!("Invalid BLE address '{}'", address));
        };

        let device = DeviceHandle {
            address: parsed,
            id: parsed.to_string(),
            name: String::new(),
        };

        println!(
            "[esp32-printer] trying direct printer address={} type={}",
            device.id,
            addr_type_label
        );

        match try_print_to_device(&device, preferred_characteristic_uuid, payload) {
            Ok(print) => {
                *cached_printer().lock().unwrap() = Some(device);
                return Ok(print);
            }
            Err(error) => {
                println!(
                    "[esp32-printer] direct printer address={} type={} failed: {}",
                    address,
                    addr_type_label,
                    error
                );
                last_error = Some(error);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        format!("Unable to connect to BLE address '{}'", address)
    }))
}

pub fn connect_printer(
    address: Option<&str>,
    address_type: Option<&str>,
    name_prefix: Option<&str>,
    preferred_characteristic_uuid: &str,
    timeout_ms: u64,
) -> Result<PrinterConnectionInfo, String> {
    let device = if let Some(address) = address {
        let mut last_error: Option<String> = None;
        let mut addr_types = Vec::new();

        if let Some(input) = address_type {
            if let Some(parsed) = parse_address_type(input) {
                addr_types.push(parsed);
            } else {
                return Err(format!("Unsupported BLE address type '{}'", input));
            }
        }

        for fallback in [
            BLEAddressType::Random,
            BLEAddressType::Public,
            BLEAddressType::RandomID,
            BLEAddressType::PublicID,
        ] {
            if !addr_types
                .iter()
                .any(|existing| address_type_label(existing) == address_type_label(&fallback))
            {
                addr_types.push(fallback);
            }
        }

        let mut connected: Option<(DeviceHandle, ConnectionHandle)> = None;
        for addr_type in addr_types {
            let label = address_type_label(&addr_type);
            let Some(parsed) = BLEAddress::from_str(address, addr_type) else {
                return Err(format!("Invalid BLE address '{}'", address));
            };
            let candidate = DeviceHandle {
                address: parsed,
                id: parsed.to_string(),
                name: String::new(),
            };

            println!(
                "[esp32-printer] trying direct printer address={} type={}",
                candidate.id, label
            );

            match connect(&candidate) {
                Ok(connection) => {
                    connected = Some((candidate, connection));
                    break;
                }
                Err(error) => {
                    println!(
                        "[esp32-printer] direct printer address={} type={} failed: {}",
                        address, label, error
                    );
                    last_error = Some(error);
                }
            }
        }

        let Some((device, connection)) = connected else {
            return Err(last_error.unwrap_or_else(|| {
                format!("Unable to connect to BLE address '{}'", address)
            }));
        };

        let services = discover_services(&connection)?;
        let (characteristic, mode) = select_characteristic(&services, preferred_characteristic_uuid)?;
        let handle_id = next_connection_id();
        let info = PrinterConnectionInfo {
            handle_id,
            device_name: if device.name.is_empty() {
                "Unknown".to_string()
            } else {
                device.name.clone()
            },
            device_id: device.id.clone(),
            write_mode: match mode {
                WriteMode::WithResponse => "withResponse".to_string(),
                WriteMode::WithoutResponse => "withoutResponse".to_string(),
            },
            characteristic_uuid: characteristic.uuid.clone(),
        };

        active_printer_connections().lock().unwrap().with_map(|connections| {
            connections.insert(
                handle_id,
                ActivePrinterConnection {
                    connection,
                    characteristic,
                    write_mode: mode,
                    device_name: info.device_name.clone(),
                    device_id: info.device_id.clone(),
                },
            );
        });
        *cached_printer().lock().unwrap() = Some(device);
        info
    } else {
        let prefix = name_prefix.unwrap_or("KM");
        let adapter = get_default_adapter()?;
        let devices = scan(
            &adapter,
            &ScanOptions {
                timeout_ms,
                services: Vec::new(),
                name_prefix: Some(prefix.to_string()),
            },
        )?;
        let device = devices
            .into_iter()
            .next()
            .ok_or_else(|| format!("No BLE printer was found with prefix '{}'", prefix))?;
        let connection = connect(&device)?;
        let services = discover_services(&connection)?;
        let (characteristic, mode) = select_characteristic(&services, preferred_characteristic_uuid)?;
        let handle_id = next_connection_id();
        let info = PrinterConnectionInfo {
            handle_id,
            device_name: if device.name.is_empty() {
                "Unknown".to_string()
            } else {
                device.name.clone()
            },
            device_id: device.id.clone(),
            write_mode: match mode {
                WriteMode::WithResponse => "withResponse".to_string(),
                WriteMode::WithoutResponse => "withoutResponse".to_string(),
            },
            characteristic_uuid: characteristic.uuid.clone(),
        };

        active_printer_connections().lock().unwrap().with_map(|connections| {
            connections.insert(
                handle_id,
                ActivePrinterConnection {
                    connection,
                    characteristic,
                    write_mode: mode,
                    device_name: info.device_name.clone(),
                    device_id: info.device_id.clone(),
                },
            );
        });
        *cached_printer().lock().unwrap() = Some(device);
        info
    };

    Ok(device)
}

pub fn write_connected(handle_id: u32, payload: &[u8]) -> Result<PrintResult, String> {
    let active = active_printer_connections().lock().unwrap().with_map(|connections| {
        connections.get(&handle_id).cloned()
    })
    .ok_or_else(|| format!("Unknown printer handle {}", handle_id))?;

    write(&active.characteristic, payload, active.write_mode)?;
    Ok(PrintResult {
        device_name: active.device_name,
        device_id: active.device_id,
        write_mode: match active.write_mode {
            WriteMode::WithResponse => "withResponse".to_string(),
            WriteMode::WithoutResponse => "withoutResponse".to_string(),
        },
        payload_bytes: payload.len(),
    })
}

pub fn disconnect_handle(handle_id: u32) -> Result<(), String> {
    let active = active_printer_connections().lock().unwrap().with_map(|connections| {
        connections.remove(&handle_id)
    })
    .ok_or_else(|| format!("Unknown printer handle {}", handle_id))?;
    disconnect(&active.connection)
}

fn try_print_to_device(
    device: &DeviceHandle,
    preferred_characteristic_uuid: &str,
    payload: &[u8],
) -> Result<PrintResult, String> {
    println!(
        "[esp32-printer] connecting to id={} name={:?}",
        device.id, device.name
    );

    let connection = connect(&device)?;
    let services = discover_services(&connection)?;
    let (characteristic, mode) = select_characteristic(&services, preferred_characteristic_uuid)?;

    let write_result = write(&characteristic, payload, mode);
    let disconnect_result = disconnect(&connection);

    if let Err(error) = write_result {
        let _ = disconnect_result;
        return Err(error);
    }
    if let Err(error) = disconnect_result {
        return Err(error);
    }

    Ok(PrintResult {
        device_name: if device.name.is_empty() {
            "Unknown".to_string()
        } else {
            device.name.clone()
        },
        device_id: device.id.clone(),
        write_mode: match mode {
            WriteMode::WithResponse => "withResponse".to_string(),
            WriteMode::WithoutResponse => "withoutResponse".to_string(),
        },
        payload_bytes: payload.len(),
    })
}

pub fn print_hello_boxlang() -> Result<PrintResult, String> {
    let payload = concat!(
        "SIZE 48 mm,24 mm\r\n",
        "GAP 2 mm,0 mm\r\n",
        "DENSITY 8\r\n",
        "SPEED 3\r\n",
        "DIRECTION 1\r\n",
        "CLS\r\n",
        "TEXT 20,20,\"3\",0,1,1,\"Hello, BoxLang\"\r\n",
        "PRINT 1,1\r\n"
    )
    .as_bytes()
    .to_vec();
    print_bytes(
        "KM",
        "00002af1-0000-1000-8000-00805f9b34fb",
        5000,
        &payload,
    )
}
