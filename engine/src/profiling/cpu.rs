use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CpuCaptureState {
    #[default]
    Idle,
    Capturing,
    Processing,
    Complete,
    Unavailable,
    Failed,
}

#[derive(Clone, Debug, Default)]
pub struct CpuFunctionHotspot {
    pub function: String,
    pub module: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub self_samples: u64,
    pub inclusive_samples: u64,
}

#[derive(Clone, Debug, Default)]
pub struct CpuSourceHotspot {
    pub file: String,
    pub self_samples: u64,
    pub inclusive_samples: u64,
}

#[derive(Clone, Debug, Default)]
pub struct CpuSourceLineHotspot {
    pub file: String,
    pub line: u32,
    pub self_samples: u64,
    pub inclusive_samples: u64,
}

#[derive(Clone, Debug, Default)]
pub struct CpuThreadHotspot {
    pub thread_id: u32,
    pub samples: u64,
}

#[derive(Clone, Debug, Default)]
pub struct CpuCallTreeNode {
    pub function: String,
    pub module: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub self_samples: u64,
    pub inclusive_samples: u64,
    pub children: Vec<CpuCallTreeNode>,
}

#[derive(Clone, Debug)]
pub struct CpuProfileSnapshot {
    pub supported: bool,
    pub state: CpuCaptureState,
    pub status: String,
    pub elapsed: Duration,
    pub requested_duration: Duration,
    pub total_samples: u64,
    pub distinct_stacks: usize,
    pub captured_frames: u64,
    pub sample_interval: Duration,
    pub symbolized_addresses: usize,
    pub unresolved_addresses: usize,
    pub functions: Vec<CpuFunctionHotspot>,
    pub source_files: Vec<CpuSourceHotspot>,
    pub source_lines: Vec<CpuSourceLineHotspot>,
    pub threads: Vec<CpuThreadHotspot>,
    pub call_tree: Vec<CpuCallTreeNode>,
    pub bottom_up: Vec<CpuCallTreeNode>,
}

impl Default for CpuProfileSnapshot {
    fn default() -> Self {
        Self {
            supported: platform::SUPPORTED,
            state: if platform::SUPPORTED {
                CpuCaptureState::Idle
            } else {
                CpuCaptureState::Unavailable
            },
            status: if platform::SUPPORTED {
                "Ready to capture CPU samples.".to_string()
            } else {
                "Automatic CPU sampling is currently supported on 64-bit Windows.".to_string()
            },
            elapsed: Duration::ZERO,
            requested_duration: Duration::from_secs(3),
            total_samples: 0,
            distinct_stacks: 0,
            captured_frames: 0,
            sample_interval: Duration::from_millis(1),
            symbolized_addresses: 0,
            unresolved_addresses: 0,
            functions: Vec::new(),
            source_files: Vec::new(),
            source_lines: Vec::new(),
            threads: Vec::new(),
            call_tree: Vec::new(),
            bottom_up: Vec::new(),
        }
    }
}

pub fn start_capture(duration: Duration) -> Result<(), String> {
    platform::start_capture(duration)
}

pub fn stop_capture() {
    platform::stop_capture();
}

pub fn clear_capture() {
    platform::clear_capture();
}

pub fn poll() {
    platform::poll();
}

pub fn mark_frame(index: u64) {
    platform::mark_frame(index);
}

pub fn snapshot() -> CpuProfileSnapshot {
    platform::snapshot()
}

#[cfg(not(all(feature = "profiling", target_os = "windows", target_arch = "x86_64")))]
mod platform {
    use super::*;

    pub const SUPPORTED: bool = false;

    pub fn start_capture(_duration: Duration) -> Result<(), String> {
        Err("Automatic CPU sampling requires the profiling feature on 64-bit Windows.".to_string())
    }

    pub fn stop_capture() {}
    pub fn clear_capture() {}
    pub fn poll() {}
    pub fn mark_frame(_index: u64) {}
    pub fn snapshot() -> CpuProfileSnapshot {
        CpuProfileSnapshot::default()
    }
}

#[cfg(all(feature = "profiling", target_os = "windows", target_arch = "x86_64"))]
mod platform {
    use std::{
        collections::{HashMap, HashSet},
        ffi::{CStr, c_void},
        hash::{Hash, Hasher},
        mem::{offset_of, size_of, zeroed},
        ptr::{null, null_mut},
        sync::{
            Arc, LazyLock, Mutex,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        thread::{self, JoinHandle},
        time::{Duration, Instant},
    };

    use rustc_demangle::try_demangle;
    use windows_sys::{
        Win32::{
            Foundation::{
                CloseHandle, ERROR_ACCESS_DENIED, ERROR_NOT_ALL_ASSIGNED, ERROR_SUCCESS,
                GetLastError, HANDLE, SetLastError,
            },
            Security::{
                AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW,
                SE_PRIVILEGE_ENABLED, SE_SYSTEM_PROFILE_NAME, TOKEN_ADJUST_PRIVILEGES,
                TOKEN_PRIVILEGES, TOKEN_QUERY,
            },
            System::{
                Diagnostics::{
                    Debug::{
                        IMAGEHLP_LINE64, IMAGEHLP_MODULE64, SYMBOL_INFO, SYMOPT_DEFERRED_LOADS,
                        SYMOPT_FAIL_CRITICAL_ERRORS, SYMOPT_LOAD_LINES, SYMOPT_NO_PROMPTS,
                        SYMOPT_UNDNAME, SymCleanup, SymFromAddr, SymGetLineFromAddr64,
                        SymGetModuleInfo64, SymInitializeW, SymRefreshModuleList, SymSetOptions,
                        SymSetSearchPathW,
                    },
                    Etw::{
                        CLASSIC_EVENT_ID, CONTROLTRACE_HANDLE, CloseTrace, ControlTraceW,
                        EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_FLAG_PROFILE,
                        EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE,
                        EVENT_TRACE_SYSTEM_LOGGER_MODE, OpenTraceW,
                        PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME,
                        PerfInfoGuid, ProcessTrace, StartTraceW, TRACE_PROFILE_INTERVAL,
                        TraceQueryInformation, TraceSampledProfileIntervalInfo,
                        TraceSetInformation, TraceStackTracingInfo, WNODE_FLAG_TRACED_GUID,
                    },
                },
                Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcessToken},
            },
        },
        core::GUID,
    };

    use super::*;

    pub const SUPPORTED: bool = true;
    const SAMPLE_PROFILE_EVENT_TYPE: u8 = 46;
    const STACK_WALK_EVENT_TYPE: u8 = 32;
    const MAX_STACK_DEPTH: usize = 192;
    const MAX_SYMBOL_NAME: usize = 2048;
    const STACK_WALK_GUID: GUID = GUID::from_u128(0xdef2fe46_7bd6_4b80_bd94_f57fe20d0ce3);

    static MANAGER: LazyLock<Mutex<Manager>> = LazyLock::new(|| {
        Mutex::new(Manager {
            snapshot: CpuProfileSnapshot::default(),
            started: None,
            stop: None,
            worker: None,
        })
    });
    static CURRENT_FRAME: AtomicU64 = AtomicU64::new(0);

    struct Manager {
        snapshot: CpuProfileSnapshot,
        started: Option<Instant>,
        stop: Option<Arc<AtomicBool>>,
        worker: Option<JoinHandle<Result<CpuProfileSnapshot, String>>>,
    }

    #[derive(Clone, Eq)]
    struct RawStack {
        thread_id: u32,
        addresses: Vec<u64>,
    }

    impl PartialEq for RawStack {
        fn eq(&self, other: &Self) -> bool {
            self.thread_id == other.thread_id && self.addresses == other.addresses
        }
    }

    impl Hash for RawStack {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.thread_id.hash(state);
            self.addresses.hash(state);
        }
    }

    #[derive(Default)]
    struct CaptureContext {
        process_id: u32,
        stacks: HashMap<RawStack, u64>,
    }

    #[repr(C)]
    struct PropertiesBuffer {
        properties: EVENT_TRACE_PROPERTIES,
        logger_name: [u16; 256],
    }

    impl PropertiesBuffer {
        fn new() -> Self {
            let mut value: Self = unsafe { zeroed() };
            value.properties.Wnode.BufferSize = size_of::<Self>() as u32;
            value.properties.LoggerNameOffset = offset_of!(Self, logger_name) as u32;
            value
        }
    }

    pub fn start_capture(duration: Duration) -> Result<(), String> {
        if duration < Duration::from_millis(250) {
            return Err("CPU captures must be at least 250 ms.".to_string());
        }
        if duration > Duration::from_secs(60) {
            return Err("CPU captures are limited to 60 seconds.".to_string());
        }

        poll();
        let mut manager = MANAGER.lock().unwrap();
        if manager.worker.is_some() {
            return Err("A CPU capture is already running.".to_string());
        }

        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("plaxel-cpu-profiler".to_string())
            .spawn(move || capture_worker(duration, worker_stop))
            .map_err(|error| format!("Could not start CPU profiler thread: {error}"))?;

        manager.snapshot = CpuProfileSnapshot {
            supported: true,
            state: CpuCaptureState::Capturing,
            status: "Capturing ETW CPU samples…".to_string(),
            requested_duration: duration,
            ..CpuProfileSnapshot::default()
        };
        manager.started = Some(Instant::now());
        manager.stop = Some(stop);
        manager.worker = Some(worker);
        Ok(())
    }

    pub fn stop_capture() {
        let manager = MANAGER.lock().unwrap();
        if let Some(stop) = &manager.stop {
            stop.store(true, Ordering::Release);
        }
    }

    pub fn clear_capture() {
        poll();
        let mut manager = MANAGER.lock().unwrap();
        if manager.worker.is_none() {
            manager.snapshot = CpuProfileSnapshot::default();
            manager.started = None;
            manager.stop = None;
        }
    }

    pub fn poll() {
        let worker = {
            let mut manager = MANAGER.lock().unwrap();
            let finished = manager.worker.as_ref().is_some_and(JoinHandle::is_finished);
            if !finished {
                return;
            }
            manager.snapshot.state = CpuCaptureState::Processing;
            manager.snapshot.status = "Resolving symbols and building call trees…".to_string();
            manager.worker.take()
        };

        let Some(worker) = worker else {
            return;
        };
        let result = worker
            .join()
            .unwrap_or_else(|_| Err("CPU profiler worker panicked.".to_string()));

        let mut manager = MANAGER.lock().unwrap();
        match result {
            Ok(snapshot) => manager.snapshot = snapshot,
            Err(error) => {
                let requested_duration = manager.snapshot.requested_duration;
                manager.snapshot = CpuProfileSnapshot {
                    supported: true,
                    state: CpuCaptureState::Failed,
                    status: error,
                    requested_duration,
                    ..CpuProfileSnapshot::default()
                };
            }
        }
        manager.started = None;
        manager.stop = None;
    }

    pub fn snapshot() -> CpuProfileSnapshot {
        poll();
        let manager = MANAGER.lock().unwrap();
        let mut snapshot = manager.snapshot.clone();
        if let Some(started) = manager.started {
            snapshot.elapsed = started.elapsed().min(snapshot.requested_duration);
        }
        snapshot
    }

    pub fn mark_frame(index: u64) {
        CURRENT_FRAME.store(index, Ordering::Relaxed);
    }

    fn capture_worker(
        duration: Duration,
        stop_requested: Arc<AtomicBool>,
    ) -> Result<CpuProfileSnapshot, String> {
        enable_system_profile_privilege()?;

        let process_id = unsafe { GetCurrentProcessId() };
        let first_frame = CURRENT_FRAME.load(Ordering::Relaxed);
        let session_name = wide_null(&format!("Plaxel CPU Profiler (PID {process_id})"));
        let mut properties = PropertiesBuffer::new();
        properties.properties.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        properties.properties.Wnode.ClientContext = 1;
        properties.properties.BufferSize = 64;
        properties.properties.MinimumBuffers = 4;
        properties.properties.MaximumBuffers = 64;
        properties.properties.FlushTimer = 1;
        properties.properties.LogFileMode =
            EVENT_TRACE_REAL_TIME_MODE | EVENT_TRACE_SYSTEM_LOGGER_MODE;
        properties.properties.EnableFlags = EVENT_TRACE_FLAG_PROFILE;

        let mut session = CONTROLTRACE_HANDLE::default();
        let start_result = unsafe {
            StartTraceW(
                &mut session,
                session_name.as_ptr(),
                &mut properties.properties,
            )
        };
        if start_result != ERROR_SUCCESS {
            return Err(etw_error(
                "Could not start the ETW CPU sampling session",
                start_result,
            ));
        }

        let profile_event = CLASSIC_EVENT_ID {
            EventGuid: PerfInfoGuid,
            Type: SAMPLE_PROFILE_EVENT_TYPE,
            Reserved: [0; 7],
        };
        let stack_result = unsafe {
            TraceSetInformation(
                session,
                TraceStackTracingInfo,
                (&profile_event as *const CLASSIC_EVENT_ID).cast(),
                size_of::<CLASSIC_EVENT_ID>() as u32,
            )
        };
        if stack_result != ERROR_SUCCESS {
            stop_session(session, &session_name);
            return Err(etw_error(
                "ETW started, but sampled call-stack collection could not be enabled",
                stack_result,
            ));
        }
        let sample_interval = query_sample_interval(session);

        let mut context = Box::new(CaptureContext {
            process_id,
            stacks: HashMap::new(),
        });
        let context_pointer = (&mut *context as *mut CaptureContext).cast::<c_void>();

        let mut logger_name = session_name.clone();
        let mut log_file: EVENT_TRACE_LOGFILEW = unsafe { zeroed() };
        log_file.LoggerName = logger_name.as_mut_ptr();
        log_file.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        log_file.Anonymous2.EventRecordCallback = Some(event_record_callback);
        log_file.Context = context_pointer;

        let trace = unsafe { OpenTraceW(&mut log_file) };
        if trace.Value == u64::MAX {
            stop_session(session, &session_name);
            return Err(
                "ETW session started, but its real-time consumer could not open.".to_string(),
            );
        }

        let stopper_name = session_name.clone();
        let stopper = thread::Builder::new()
            .name("plaxel-cpu-profiler-stop".to_string())
            .spawn(move || {
                let started = Instant::now();
                while started.elapsed() < duration && !stop_requested.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(20));
                }
                stop_session(session, &stopper_name);
            })
            .map_err(|error| {
                unsafe {
                    CloseTrace(trace);
                }
                stop_session(session, &session_name);
                format!("Could not start ETW capture timer: {error}")
            })?;

        let capture_started = Instant::now();
        let process_result = unsafe { ProcessTrace(&trace, 1, null(), null()) };
        unsafe {
            CloseTrace(trace);
        }
        let _ = stopper.join();

        if process_result != ERROR_SUCCESS {
            // Stopping a real-time session normally wakes ProcessTrace with a
            // non-success status on some Windows versions. A completed capture
            // with samples is still valid.
            if context.stacks.is_empty() {
                return Err(etw_error(
                    "The ETW real-time consumer stopped without producing samples",
                    process_result,
                ));
            }
        }

        build_snapshot(
            context.stacks,
            capture_started.elapsed().min(duration),
            duration,
            CURRENT_FRAME
                .load(Ordering::Relaxed)
                .saturating_sub(first_frame)
                .max(1),
            sample_interval,
        )
    }

    fn stop_session(session: CONTROLTRACE_HANDLE, session_name: &[u16]) {
        let mut properties = PropertiesBuffer::new();
        unsafe {
            ControlTraceW(
                session,
                session_name.as_ptr(),
                &mut properties.properties,
                EVENT_TRACE_CONTROL_STOP,
            );
        }
    }

    fn query_sample_interval(session: CONTROLTRACE_HANDLE) -> Duration {
        let mut profile_interval = TRACE_PROFILE_INTERVAL::default();
        let mut returned = 0u32;
        let result = unsafe {
            TraceQueryInformation(
                session,
                TraceSampledProfileIntervalInfo,
                (&mut profile_interval as *mut TRACE_PROFILE_INTERVAL).cast(),
                size_of::<TRACE_PROFILE_INTERVAL>() as u32,
                &mut returned,
            )
        };
        if result == ERROR_SUCCESS && profile_interval.Interval > 0 {
            Duration::from_nanos(profile_interval.Interval as u64 * 100)
        } else {
            // Windows' default timer profile interval is 10,000 x 100 ns.
            Duration::from_millis(1)
        }
    }

    fn enable_system_profile_privilege() -> Result<(), String> {
        unsafe {
            let mut token = null_mut();
            if OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            ) == 0
            {
                return Err(format!(
                    "Could not open the process token to enable ETW profiling privilege (Windows error {}).",
                    GetLastError()
                ));
            }

            let mut luid = zeroed();
            if LookupPrivilegeValueW(null(), SE_SYSTEM_PROFILE_NAME, &mut luid) == 0 {
                let error = GetLastError();
                CloseHandle(token);
                return Err(format!(
                    "Could not look up SeSystemProfilePrivilege (Windows error {error})."
                ));
            }

            let privileges = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };

            // AdjustTokenPrivileges may return success while setting
            // ERROR_NOT_ALL_ASSIGNED, so the last error must be checked too.
            SetLastError(ERROR_SUCCESS);
            let adjusted = AdjustTokenPrivileges(token, 0, &privileges, 0, null_mut(), null_mut());
            let error = GetLastError();
            CloseHandle(token);

            if adjusted == 0 {
                return Err(format!(
                    "Could not enable SeSystemProfilePrivilege (Windows error {error})."
                ));
            }
            if error == ERROR_NOT_ALL_ASSIGNED {
                return Err(
                    "This process token does not contain SeSystemProfilePrivilege. Start the editor from a Windows terminal opened with “Run as administrator”."
                        .to_string(),
                );
            }
        }
        Ok(())
    }

    unsafe extern "system" fn event_record_callback(event_record: *mut EVENT_RECORD) {
        if event_record.is_null() {
            return;
        }
        let event = unsafe { &*event_record };
        if !guid_eq(event.EventHeader.ProviderId, STACK_WALK_GUID)
            || event.EventHeader.EventDescriptor.Opcode != STACK_WALK_EVENT_TYPE
            || event.UserData.is_null()
            || event.UserContext.is_null()
        {
            return;
        }

        // StackWalk_Event on x64:
        // u64 original_timestamp, u32 process_id, u32 thread_id, u64 addresses[].
        let length = event.UserDataLength as usize;
        if length < 24 {
            return;
        }
        let data = event.UserData.cast::<u8>();
        let process_id = unsafe { data.add(8).cast::<u32>().read_unaligned() };
        let thread_id = unsafe { data.add(12).cast::<u32>().read_unaligned() };
        let context = unsafe { &mut *event.UserContext.cast::<CaptureContext>() };
        if process_id != context.process_id {
            return;
        }

        let address_count = ((length - 16) / size_of::<u64>()).min(MAX_STACK_DEPTH);
        let mut addresses = (0..address_count)
            .map(|index| unsafe {
                data.add(16 + index * size_of::<u64>())
                    .cast::<u64>()
                    .read_unaligned()
            })
            .filter(|address| *address != 0)
            .collect::<Vec<_>>();
        addresses.dedup();
        if addresses.is_empty() {
            return;
        }

        *context
            .stacks
            .entry(RawStack {
                thread_id,
                addresses,
            })
            .or_insert(0) += 1;
    }

    #[derive(Clone)]
    struct ResolvedFrame {
        key: u64,
        function: String,
        module: String,
        file: Option<String>,
        line: Option<u32>,
        symbolized: bool,
    }

    struct Symbolizer {
        process: HANDLE,
        cache: HashMap<u64, ResolvedFrame>,
    }

    impl Symbolizer {
        fn new() -> Result<Self, String> {
            let process = unsafe { GetCurrentProcess() };
            let search_path = symbol_search_path();
            unsafe {
                SymSetOptions(
                    SYMOPT_DEFERRED_LOADS
                        | SYMOPT_LOAD_LINES
                        | SYMOPT_UNDNAME
                        | SYMOPT_FAIL_CRITICAL_ERRORS
                        | SYMOPT_NO_PROMPTS,
                );
                if SymInitializeW(process, search_path.as_ptr(), 1) == 0 {
                    return Err(
                        "DbgHelp could not initialize symbol resolution. Ensure matching PDB files are available in the Cargo target directory."
                            .to_string(),
                    );
                }
                SymSetSearchPathW(process, search_path.as_ptr());
                SymRefreshModuleList(process);
            }
            Ok(Self {
                process,
                cache: HashMap::new(),
            })
        }

        fn resolve(&mut self, address: u64) -> ResolvedFrame {
            if let Some(frame) = self.cache.get(&address) {
                return frame.clone();
            }

            let module = self.module(address);
            let mut symbol_storage =
                vec![0u64; (size_of::<SYMBOL_INFO>() + MAX_SYMBOL_NAME + 7) / 8];
            let symbol = symbol_storage.as_mut_ptr().cast::<SYMBOL_INFO>();
            let mut displacement = 0u64;
            let (key, function, symbolized) = unsafe {
                (*symbol).SizeOfStruct = size_of::<SYMBOL_INFO>() as u32;
                (*symbol).MaxNameLen = MAX_SYMBOL_NAME as u32;
                if SymFromAddr(self.process, address, &mut displacement, symbol) != 0 {
                    let raw_name = CStr::from_ptr((*symbol).Name.as_ptr())
                        .to_string_lossy()
                        .into_owned();
                    let demangled = try_demangle(&raw_name)
                        .map(|name| name.to_string())
                        .unwrap_or(raw_name);
                    ((*symbol).Address, demangled, true)
                } else {
                    (
                        address,
                        if module.is_empty() {
                            format!("0x{address:016X}")
                        } else {
                            format!("{module}!0x{address:016X}")
                        },
                        false,
                    )
                }
            };
            let (file, line) = self.source_line(address);
            let frame = ResolvedFrame {
                key,
                function,
                module,
                file,
                line,
                symbolized,
            };
            self.cache.insert(address, frame.clone());
            frame
        }

        fn module(&self, address: u64) -> String {
            unsafe {
                let mut info: IMAGEHLP_MODULE64 = zeroed();
                info.SizeOfStruct = size_of::<IMAGEHLP_MODULE64>() as u32;
                if SymGetModuleInfo64(self.process, address, &mut info) == 0 {
                    return String::new();
                }
                CStr::from_ptr(info.ModuleName.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            }
        }

        fn source_line(&self, address: u64) -> (Option<String>, Option<u32>) {
            unsafe {
                let mut info: IMAGEHLP_LINE64 = zeroed();
                info.SizeOfStruct = size_of::<IMAGEHLP_LINE64>() as u32;
                let mut displacement = 0u32;
                if SymGetLineFromAddr64(self.process, address, &mut displacement, &mut info) == 0
                    || info.FileName.is_null()
                {
                    return (None, None);
                }
                (
                    Some(
                        CStr::from_ptr(info.FileName.cast())
                            .to_string_lossy()
                            .into_owned(),
                    ),
                    Some(info.LineNumber),
                )
            }
        }
    }

    impl Drop for Symbolizer {
        fn drop(&mut self) {
            unsafe {
                SymCleanup(self.process);
            }
        }
    }

    #[derive(Default)]
    struct FunctionAccumulator {
        frame: Option<ResolvedFrame>,
        self_samples: u64,
        inclusive_samples: u64,
    }

    #[derive(Default)]
    struct SourceAccumulator {
        self_samples: u64,
        inclusive_samples: u64,
    }

    struct TreeAccumulator {
        frame: ResolvedFrame,
        self_samples: u64,
        inclusive_samples: u64,
        children: HashMap<u64, TreeAccumulator>,
    }

    fn build_snapshot(
        stacks: HashMap<RawStack, u64>,
        elapsed: Duration,
        requested_duration: Duration,
        captured_frames: u64,
        sample_interval: Duration,
    ) -> Result<CpuProfileSnapshot, String> {
        let distinct_stacks = stacks.len();
        let total_samples = stacks.values().sum();
        let mut symbolizer = Symbolizer::new()?;
        let mut functions = HashMap::<u64, FunctionAccumulator>::new();
        let mut sources = HashMap::<String, SourceAccumulator>::new();
        let mut source_lines = HashMap::<(String, u32), SourceAccumulator>::new();
        let mut threads = HashMap::<u32, u64>::new();
        let mut tree = HashMap::<u64, TreeAccumulator>::new();
        let mut bottom_up = HashMap::<u64, TreeAccumulator>::new();

        for (stack, count) in stacks {
            *threads.entry(stack.thread_id).or_insert(0) += count;
            let frames = stack
                .addresses
                .into_iter()
                .map(|address| symbolizer.resolve(address))
                .collect::<Vec<_>>();
            let Some(leaf) = frames.first() else {
                continue;
            };

            let leaf_cost = functions.entry(leaf.key).or_default();
            leaf_cost.frame.get_or_insert_with(|| leaf.clone());
            leaf_cost.self_samples += count;
            if let Some(file) = &leaf.file {
                sources.entry(file.clone()).or_default().self_samples += count;
                if let Some(line) = leaf.line {
                    source_lines
                        .entry((file.clone(), line))
                        .or_default()
                        .self_samples += count;
                }
            }

            let mut seen_functions = HashSet::new();
            let mut seen_sources = HashSet::new();
            let mut seen_source_lines = HashSet::new();
            for frame in &frames {
                if seen_functions.insert(frame.key) {
                    let cost = functions.entry(frame.key).or_default();
                    cost.frame.get_or_insert_with(|| frame.clone());
                    cost.inclusive_samples += count;
                }
                if let Some(file) = &frame.file {
                    if seen_sources.insert(file.clone()) {
                        sources.entry(file.clone()).or_default().inclusive_samples += count;
                    }
                    if let Some(line) = frame.line {
                        if seen_source_lines.insert((file.clone(), line)) {
                            source_lines
                                .entry((file.clone(), line))
                                .or_default()
                                .inclusive_samples += count;
                        }
                    }
                }
            }

            let root_to_leaf = frames.into_iter().rev().collect::<Vec<_>>();
            add_tree_sample(&mut tree, &root_to_leaf, count);
            let leaf_to_root = root_to_leaf.into_iter().rev().collect::<Vec<_>>();
            add_tree_sample(&mut bottom_up, &leaf_to_root, count);
        }

        let symbolized_addresses = symbolizer
            .cache
            .values()
            .filter(|frame| frame.symbolized)
            .count();
        let unresolved_addresses = symbolizer.cache.len().saturating_sub(symbolized_addresses);

        let mut function_rows = functions
            .into_values()
            .filter_map(|cost| {
                let frame = cost.frame?;
                Some(CpuFunctionHotspot {
                    function: frame.function,
                    module: frame.module,
                    file: frame.file,
                    line: frame.line,
                    self_samples: cost.self_samples,
                    inclusive_samples: cost.inclusive_samples,
                })
            })
            .collect::<Vec<_>>();
        function_rows.sort_unstable_by(|a, b| {
            b.self_samples
                .cmp(&a.self_samples)
                .then_with(|| b.inclusive_samples.cmp(&a.inclusive_samples))
        });

        let mut source_rows = sources
            .into_iter()
            .map(|(file, cost)| CpuSourceHotspot {
                file,
                self_samples: cost.self_samples,
                inclusive_samples: cost.inclusive_samples,
            })
            .collect::<Vec<_>>();
        source_rows.sort_unstable_by(|a, b| {
            b.self_samples
                .cmp(&a.self_samples)
                .then_with(|| b.inclusive_samples.cmp(&a.inclusive_samples))
        });

        let mut source_line_rows = source_lines
            .into_iter()
            .map(|((file, line), cost)| CpuSourceLineHotspot {
                file,
                line,
                self_samples: cost.self_samples,
                inclusive_samples: cost.inclusive_samples,
            })
            .collect::<Vec<_>>();
        source_line_rows.sort_unstable_by(|a, b| {
            b.self_samples
                .cmp(&a.self_samples)
                .then_with(|| b.inclusive_samples.cmp(&a.inclusive_samples))
        });

        let mut thread_rows = threads
            .into_iter()
            .map(|(thread_id, samples)| CpuThreadHotspot { thread_id, samples })
            .collect::<Vec<_>>();
        thread_rows.sort_unstable_by(|a, b| b.samples.cmp(&a.samples));

        Ok(CpuProfileSnapshot {
            supported: true,
            state: CpuCaptureState::Complete,
            status: format!(
                "Capture complete: {total_samples} samples, {symbolized_addresses} symbolized addresses, {unresolved_addresses} unresolved."
            ),
            elapsed,
            requested_duration,
            total_samples,
            distinct_stacks,
            captured_frames,
            sample_interval,
            symbolized_addresses,
            unresolved_addresses,
            functions: function_rows,
            source_files: source_rows,
            source_lines: source_line_rows,
            threads: thread_rows,
            call_tree: finish_tree(tree),
            bottom_up: finish_tree(bottom_up),
        })
    }

    fn add_tree_sample(
        nodes: &mut HashMap<u64, TreeAccumulator>,
        frames: &[ResolvedFrame],
        count: u64,
    ) {
        let Some((frame, rest)) = frames.split_first() else {
            return;
        };
        let node = nodes.entry(frame.key).or_insert_with(|| TreeAccumulator {
            frame: frame.clone(),
            self_samples: 0,
            inclusive_samples: 0,
            children: HashMap::new(),
        });
        node.inclusive_samples += count;
        if rest.is_empty() {
            node.self_samples += count;
        } else {
            add_tree_sample(&mut node.children, rest, count);
        }
    }

    fn finish_tree(nodes: HashMap<u64, TreeAccumulator>) -> Vec<CpuCallTreeNode> {
        let mut result = nodes
            .into_values()
            .map(|node| CpuCallTreeNode {
                function: node.frame.function,
                module: node.frame.module,
                file: node.frame.file,
                line: node.frame.line,
                self_samples: node.self_samples,
                inclusive_samples: node.inclusive_samples,
                children: finish_tree(node.children),
            })
            .collect::<Vec<_>>();
        result.sort_unstable_by(|a, b| b.inclusive_samples.cmp(&a.inclusive_samples));
        result
    }

    fn wide_null(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    fn symbol_search_path() -> Vec<u16> {
        let mut paths = Vec::<String>::new();
        if let Ok(executable) = std::env::current_exe() {
            if let Some(directory) = executable.parent() {
                paths.push(directory.to_string_lossy().into_owned());
                paths.push(directory.join("deps").to_string_lossy().into_owned());
            }
        }
        if let Ok(directory) = std::env::current_dir() {
            paths.push(directory.to_string_lossy().into_owned());
        }
        for variable in ["_NT_ALT_SYMBOL_PATH", "_NT_SYMBOL_PATH"] {
            if let Ok(path) = std::env::var(variable) {
                if !path.is_empty() {
                    paths.push(path);
                }
            }
        }
        wide_null(&paths.join(";"))
    }

    fn guid_eq(left: GUID, right: GUID) -> bool {
        left.data1 == right.data1
            && left.data2 == right.data2
            && left.data3 == right.data3
            && left.data4 == right.data4
    }

    fn etw_error(context: &str, code: u32) -> String {
        if code == ERROR_ACCESS_DENIED {
            format!(
                "{context} (Windows error {code}: access denied). Run the editor elevated; ETW sampled profiling requires Windows system-profile privileges."
            )
        } else {
            format!("{context} (Windows error {code}).")
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[inline(never)]
        fn symbol_resolution_probe() {
            std::hint::black_box(());
        }

        #[test]
        fn resolves_function_and_source_from_cargo_deps_pdb() {
            symbol_resolution_probe();
            let mut symbolizer = Symbolizer::new().unwrap();
            let frame = symbolizer.resolve(symbol_resolution_probe as *const () as usize as u64);
            assert!(frame.symbolized, "unresolved frame: {}", frame.function);
            assert!(
                frame.file.is_some(),
                "resolved function without source: {}",
                frame.function
            );
        }

        #[test]
        #[ignore = "requires permission to start an ETW system logger"]
        fn captures_and_symbolizes_busy_cpu_work() {
            start_capture(Duration::from_millis(750)).unwrap();
            let started = Instant::now();
            let mut value = 0.0_f64;
            while started.elapsed() < Duration::from_secs(2) {
                for index in 0..20_000 {
                    value += (index as f64).sin().cos();
                }
                std::hint::black_box(value);
                poll();
                let snapshot = snapshot();
                if matches!(
                    snapshot.state,
                    CpuCaptureState::Complete | CpuCaptureState::Failed
                ) {
                    assert_eq!(
                        snapshot.state,
                        CpuCaptureState::Complete,
                        "{}",
                        snapshot.status
                    );
                    assert!(snapshot.total_samples > 0);
                    assert!(!snapshot.functions.is_empty());
                    return;
                }
            }
            panic!("ETW capture did not finish within the smoke-test timeout");
        }
    }
}
