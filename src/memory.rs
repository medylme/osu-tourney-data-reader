use serde::{Deserialize, Serialize};
#[cfg(target_os = "windows")]
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::sync::Arc;
use tokio::sync::RwLock;

macro_rules! log_info {
    ($($arg:tt)*) => { log::info!("[memory] {}", format!($($arg)*)) };
}
macro_rules! log_debug {
    ($($arg:tt)*) => { log::debug!("[memory] {}", format!($($arg)*)) };
}
macro_rules! log_warn {
    ($($arg:tt)*) => { log::warn!("[memory] {}", format!($($arg)*)) };
}
macro_rules! log_error {
    ($($arg:tt)*) => { log::error!("[memory] {}", format!($($arg)*)) };
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum MemoryError {
    ProcessNotFound,
    ReadFailed(String),
    InvalidString,
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::ProcessNotFound => write!(f, "Process not found"),
            MemoryError::ReadFailed(s) => write!(f, "Read failed: {}", s),
            MemoryError::InvalidString => write!(f, "Invalid string"),
        }
    }
}

impl std::error::Error for MemoryError {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameplayMods {
    pub mods: Vec<ModInfo>,
    pub mods_string: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModInfo {
    pub acronym: String,
    pub settings: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Beatmap {
    pub id: i32,
}

#[cfg(target_os = "windows")]
mod windows_memory {
    use super::MemoryError;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows::Win32::System::Memory::{
        VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READ,
        PAGE_EXECUTE_READWRITE, PAGE_READONLY, PAGE_READWRITE,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    #[derive(Clone)]
    pub struct ProcessMemory {
        handle: HANDLE,
    }

    // safety: we're careful to only use the handle for reading
    unsafe impl Send for ProcessMemory {}
    unsafe impl Sync for ProcessMemory {}

    impl ProcessMemory {
        pub fn new(pid: u32) -> Result<Self, MemoryError> {
            let handle = unsafe {
                OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, false, pid)
                    .map_err(|e| MemoryError::ReadFailed(format!("OpenProcess failed: {}", e)))?
            };
            Ok(Self { handle })
        }

        pub fn read_bytes(&self, addr: usize, size: usize) -> Result<Vec<u8>, MemoryError> {
            let mut buffer = vec![0u8; size];
            let mut bytes_read = 0usize;
            unsafe {
                ReadProcessMemory(
                    self.handle,
                    addr as *const _,
                    buffer.as_mut_ptr() as *mut _,
                    size,
                    Some(&mut bytes_read),
                )
                .map_err(|e| MemoryError::ReadFailed(format!("ReadProcessMemory failed: {}", e)))?;
            }
            Ok(buffer)
        }

        pub fn read_ptr32(&self, addr: usize) -> Result<usize, MemoryError> {
            let bytes = self.read_bytes(addr, 4)?;
            Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
        }

        pub fn read_i32(&self, addr: usize) -> Result<i32, MemoryError> {
            let bytes = self.read_bytes(addr, 4)?;
            Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }

        pub fn read_i16(&self, addr: usize) -> Result<i16, MemoryError> {
            let bytes = self.read_bytes(addr, 2)?;
            Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
        }

        pub fn read_u16(&self, addr: usize) -> Result<u16, MemoryError> {
            let bytes = self.read_bytes(addr, 2)?;
            Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
        }

        pub fn read_i8(&self, addr: usize) -> Result<i8, MemoryError> {
            let bytes = self.read_bytes(addr, 1)?;
            Ok(bytes[0] as i8)
        }

        pub fn read_i64(&self, addr: usize) -> Result<i64, MemoryError> {
            let bytes = self.read_bytes(addr, 8)?;
            Ok(i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]))
        }

        pub fn read_f64(&self, addr: usize) -> Result<f64, MemoryError> {
            let bytes = self.read_bytes(addr, 8)?;
            Ok(f64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]))
        }

        pub fn pattern_scan(&self, pattern: &[u8], mask: &[bool]) -> Result<usize, MemoryError> {
            let mut addr: usize = 0;
            let mut mbi = MEMORY_BASIC_INFORMATION::default();
            let mut regions_scanned = 0u32;
            let mut bytes_scanned = 0usize;

            while unsafe {
                VirtualQueryEx(
                    self.handle,
                    Some(addr as *const _),
                    &mut mbi,
                    std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            } != 0
            {
                let protect = mbi.Protect;
                let is_readable = protect == PAGE_READONLY
                    || protect == PAGE_READWRITE
                    || protect == PAGE_EXECUTE_READ
                    || protect == PAGE_EXECUTE_READWRITE;

                if mbi.State == MEM_COMMIT && is_readable && mbi.RegionSize > 0 {
                    if let Ok(region) = self.read_bytes(mbi.BaseAddress as usize, mbi.RegionSize) {
                        regions_scanned += 1;
                        bytes_scanned += region.len();
                        if let Some(offset) = find_pattern(&region, pattern, mask) {
                            return Ok(mbi.BaseAddress as usize + offset);
                        }
                    }
                }

                addr = mbi.BaseAddress as usize + mbi.RegionSize;
                if addr == 0 {
                    break;
                }
            }

            log_debug!(
                "Pattern scan complete: {} regions, {} bytes - pattern not found",
                regions_scanned,
                bytes_scanned
            );
            Err(MemoryError::ReadFailed("Pattern not found".into()))
        }
    }

    impl Drop for ProcessMemory {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }

    fn find_pattern(data: &[u8], pattern: &[u8], mask: &[bool]) -> Option<usize> {
        if pattern.len() != mask.len() || pattern.is_empty() {
            return None;
        }
        'outer: for i in 0..data.len().saturating_sub(pattern.len()) {
            for j in 0..pattern.len() {
                if mask[j] && data[i + j] != pattern[j] {
                    continue 'outer;
                }
            }
            return Some(i);
        }
        None
    }
}

#[cfg(target_os = "windows")]
pub use windows_memory::ProcessMemory;

#[cfg(not(target_os = "windows"))]
#[derive(Clone)]
pub struct ProcessMemory;

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
impl ProcessMemory {
    pub fn new(_pid: u32) -> Result<Self, MemoryError> {
        Err(MemoryError::ReadFailed("Windows only".into()))
    }
    pub fn read_ptr32(&self, _: usize) -> Result<usize, MemoryError> {
        unimplemented!()
    }
    pub fn read_i32(&self, _: usize) -> Result<i32, MemoryError> {
        unimplemented!()
    }
    pub fn read_i16(&self, _: usize) -> Result<i16, MemoryError> {
        unimplemented!()
    }
    pub fn read_u16(&self, _: usize) -> Result<u16, MemoryError> {
        unimplemented!()
    }
    pub fn read_i8(&self, _: usize) -> Result<i8, MemoryError> {
        unimplemented!()
    }
    pub fn read_i64(&self, _: usize) -> Result<i64, MemoryError> {
        unimplemented!()
    }
    pub fn read_f64(&self, _: usize) -> Result<f64, MemoryError> {
        unimplemented!()
    }
    pub fn pattern_scan(&self, _: &[u8], _: &[bool]) -> Result<usize, MemoryError> {
        unimplemented!()
    }
}

pub fn parse_pattern(pattern_str: &str) -> (Vec<u8>, Vec<bool>) {
    let parts: Vec<&str> = pattern_str.split_whitespace().collect();
    let mut pattern = Vec::with_capacity(parts.len());
    let mut mask = Vec::with_capacity(parts.len());

    for part in parts {
        if part == "??" || part == "?" {
            pattern.push(0);
            mask.push(false);
        } else if let Ok(byte) = u8::from_str_radix(part, 16) {
            pattern.push(byte);
            mask.push(true);
        }
    }

    (pattern, mask)
}

pub fn order_mods(mods_str: &str) -> String {
    let order = [
        "EZ", "NF", "HT", "HR", "SD", "PF", "DT", "NC", "HD", "FL", "RX", "AP", "SO", "TD",
    ];
    let mut result = String::new();
    for m in order {
        if mods_str.contains(m) {
            result.push_str(m);
        }
    }
    if result.is_empty() {
        "NM".to_string()
    } else {
        result
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TourneyState {
    Scanning,
    Connected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TourneyData {
    pub state: TourneyState,
    pub beatmap: Beatmap,
    pub clients: Vec<TourneyClient>,
}

impl Default for TourneyData {
    fn default() -> Self {
        Self {
            state: TourneyState::Scanning,
            beatmap: Beatmap::default(),
            clients: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TourneyClient {
    pub slot: i32,
    pub team: i32, // 0 = left/red | 1 = right/blue
    pub score: i32,
    pub mods: GameplayMods,
}

#[derive(Debug, Clone, Deserialize)]
struct Offsets {
    patterns: PatternOffsets,
    base: BaseOffsets,
    beatmap: BeatmapOffsets,
    ruleset: RulesetOffsets,
}

#[derive(Debug, Clone, Deserialize)]
struct PatternOffsets {
    base: String,
    rulesets: String,
}

#[derive(Debug, Clone, Deserialize)]
struct BaseOffsets {
    beatmap_ptr: isize,
}

#[derive(Debug, Clone, Deserialize)]
struct BeatmapOffsets {
    map_id: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct RulesetOffsets {
    ptr_offset: isize,
    ptr_deref: usize,
    gameplay_ptr: usize,
    gameplay_base: usize,
    gameplay_score: usize,
    mode: usize,
    mods_ptr: usize,
    mods_xor1: usize,
    mods_xor2: usize,
}

fn load_offsets() -> Result<Offsets, String> {
    let json = include_str!("../assets/offsets.json");
    serde_json::from_str(json).map_err(|e| format!("Failed to parse offsets.json: {}", e))
}

#[derive(Clone)]
struct TourneyClientProcess {
    slot: i32,
    pid: u32,
    process: ProcessMemory,
    rulesets_addr: usize,
    base_addr: usize,
}

pub struct TourneyReader {
    offsets: Option<Offsets>,
    standalone: bool,
    clients: RwLock<Vec<TourneyClientProcess>>,
    data: RwLock<TourneyData>,
}

impl TourneyReader {
    pub fn new(standalone: bool) -> Self {
        let offsets = match load_offsets() {
            Ok(o) => {
                log_debug!("Successfully loaded offsets");
                Some(o)
            }
            Err(e) => {
                log_error!("Failed to load offsets: {}", e);
                None
            }
        };

        Self {
            offsets,
            standalone,
            clients: RwLock::new(Vec::new()),
            data: RwLock::new(TourneyData::default()),
        }
    }

    fn resolve_client_pids(&self, pids: &[u32]) -> Result<(Vec<(i32, u32)>, bool), MemoryError> {
        let tourney_err = match Self::identify_tourney_processes(pids) {
            Ok((_, clients)) if !clients.is_empty() => return Ok((clients, false)),
            Ok(_) => MemoryError::ProcessNotFound,
            Err(e) => e,
        };

        if self.standalone {
            let pid = *pids.first().ok_or(MemoryError::ProcessNotFound)?;
            Ok((vec![(0, pid)], true))
        } else {
            Err(tourney_err)
        }
    }

    pub async fn try_connect(&self) -> bool {
        let offsets = match &self.offsets {
            Some(o) => o,
            None => {
                log_debug!("Cannot connect: offsets not loaded");
                return false;
            }
        };

        let pids = match Self::find_osu_processes() {
            Ok(p) => p,
            Err(e) => {
                log_debug!("No osu! processes found: {}", e);
                return false;
            }
        };

        if pids.is_empty() {
            log_debug!("No osu! processes running");
            return false;
        }

        log_debug!("Found {} osu! process(es): {:?}", pids.len(), pids);

        let (clients, standalone_fallback) = match self.resolve_client_pids(&pids) {
            Ok(r) => r,
            Err(e) => {
                log_debug!("Tournament not detected: {}", e);
                return false;
            }
        };

        if standalone_fallback {
            log_warn!(
                "No tournament detected - attaching to osu! PID {} as slot 0 (standalone debug mode)",
                clients[0].1
            );
        } else {
            log_debug!("Tournament clients: {:?}", clients);
        }

        let mut new_clients = Vec::new();
        for (slot, pid) in clients {
            match Self::init_client_process(pid, slot, offsets) {
                Ok(client) => new_clients.push(client),
                Err(e) => log_error!("Failed to initialize client slot {}: {}", slot, e),
            }
        }

        if new_clients.is_empty() {
            log_warn!("Found tournament clients but failed to initialize any");
            return false;
        }

        let client_count = new_clients.len();
        {
            let mut clients = self.clients.write().await;
            let mut data = self.data.write().await;
            *clients = new_clients;
            data.state = TourneyState::Connected;
            data.clients = vec![TourneyClient::default(); clients.len()];
        }

        log_info!("Attached to {} tournament client instance(s)", client_count);
        true
    }

    pub async fn disconnect(&self) {
        let mut clients = self.clients.write().await;
        let mut data = self.data.write().await;
        clients.clear();
        data.state = TourneyState::Scanning;
        data.clients.clear();
        log_debug!("Disconnected from tournament clients");
    }

    pub async fn validate_clients(&self) -> bool {
        let clients = self.clients.read().await;
        if clients.is_empty() {
            return true;
        }

        let pids = match Self::find_osu_processes() {
            Ok(p) => p,
            Err(_) => return false,
        };

        let (current_clients, _) = match self.resolve_client_pids(&pids) {
            Ok(r) => r,
            Err(_) => return false,
        };

        if current_clients.len() != clients.len() {
            log_debug!(
                "Client count changed: {} -> {}",
                clients.len(),
                current_clients.len()
            );
            return false;
        }

        for (i, client) in clients.iter().enumerate() {
            let current_pid = current_clients.get(i).map(|(_, pid)| *pid);
            if current_pid != Some(client.pid) {
                log_debug!(
                    "Client {} PID changed: {} -> {:?}",
                    client.slot,
                    client.pid,
                    current_pid
                );
                return false;
            }
        }

        true
    }

    #[cfg(target_os = "windows")]
    fn find_osu_processes() -> Result<Vec<u32>, MemoryError> {
        use windows::Win32::System::ProcessStatus::{EnumProcesses, GetModuleBaseNameW};
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
        };

        let mut pids = [0u32; 1024];
        let mut bytes_returned = 0u32;

        unsafe {
            EnumProcesses(
                pids.as_mut_ptr(),
                (pids.len() * 4) as u32,
                &mut bytes_returned,
            )
            .map_err(|_| MemoryError::ProcessNotFound)?;
        }

        let num_pids = bytes_returned as usize / 4;
        let mut osu_pids = Vec::new();

        for &pid in &pids[..num_pids] {
            if pid == 0 {
                continue;
            }
            unsafe {
                if let Ok(handle) =
                    OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)
                {
                    let mut name = [0u16; 260];
                    let len = GetModuleBaseNameW(handle, None, &mut name);
                    if len > 0 {
                        let name = String::from_utf16_lossy(&name[..len as usize]);
                        if name.to_lowercase() == "osu!.exe" {
                            osu_pids.push(pid);
                        }
                    }
                }
            }
        }

        // drop lazer instances
        let titles = Self::enumerate_osu_windows();
        osu_pids.retain(|pid| {
            if titles.get(pid).is_some_and(|t| Self::is_lazer_title(t)) {
                log_debug!("Ignoring lazer instance (PID {})", pid);
                false
            } else {
                true
            }
        });

        Ok(osu_pids)
    }

    #[cfg(not(target_os = "windows"))]
    fn find_osu_processes() -> Result<Vec<u32>, MemoryError> {
        Err(MemoryError::ReadFailed("Windows only".into()))
    }

    #[cfg(target_os = "windows")]
    fn enumerate_osu_windows() -> HashMap<u32, String> {
        use std::sync::Mutex;
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowTextW, GetWindowThreadProcessId,
        };

        let results: Arc<Mutex<HashMap<u32, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let results_clone = results.clone();

        unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let results = &*(lparam.0 as *const Mutex<HashMap<u32, String>>);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let mut title = [0u16; 256];
            let len = GetWindowTextW(hwnd, &mut title);
            if len > 0 {
                let title = String::from_utf16_lossy(&title[..len as usize]);
                if title.contains("osu!") || title.contains("Tournament") {
                    if let Ok(mut map) = results.lock() {
                        // prioritize "Tournament Client" titles - don't overwrite them
                        let dominated_by_existing = map
                            .get(&pid)
                            .is_some_and(|existing| existing.contains("Tournament Client"));
                        if !dominated_by_existing {
                            map.insert(pid, title);
                        }
                    }
                }
            }
            BOOL(1)
        }

        unsafe {
            let _ = EnumWindows(
                Some(enum_callback),
                LPARAM(&*results_clone as *const _ as isize),
            );
        }

        let window_map = results.lock().unwrap().clone();
        window_map
    }

    #[cfg(target_os = "windows")]
    fn is_lazer_title(title: &str) -> bool {
        title.to_lowercase().contains("[tournament client]")
    }

    #[cfg(target_os = "windows")]
    fn identify_tourney_processes(pids: &[u32]) -> Result<(u32, Vec<(i32, u32)>), MemoryError> {
        let window_map = Self::enumerate_osu_windows();
        log_debug!("Window titles: {:?}", window_map);

        let mut manager_pid = None;
        let mut client_map: HashMap<i32, u32> = HashMap::new();

        for &pid in pids {
            if let Some(title) = window_map.get(&pid) {
                let title = title.trim();
                if let Some(rest) = title.strip_prefix("Tournament Client ") {
                    if let Ok(slot) = rest.trim().parse::<i32>() {
                        if let Some(existing_pid) = client_map.insert(slot, pid) {
                            log_error!(
                                "Duplicate Tournament Client {}: PIDs {} and {}",
                                slot,
                                existing_pid,
                                pid
                            );
                            return Err(MemoryError::ReadFailed(format!(
                                "Duplicate Tournament Client {}",
                                slot
                            )));
                        }
                    }
                } else if title.starts_with("GDI+ Window") {
                    manager_pid = Some(pid);
                }
            }
        }

        let manager = manager_pid.ok_or(MemoryError::ProcessNotFound)?;
        let mut clients: Vec<(i32, u32)> = client_map.into_iter().collect();
        clients.sort_by_key(|(slot, _)| *slot);

        Ok((manager, clients))
    }

    #[cfg(not(target_os = "windows"))]
    fn identify_tourney_processes(_: &[u32]) -> Result<(u32, Vec<(i32, u32)>), MemoryError> {
        Err(MemoryError::ReadFailed("Windows only".into()))
    }

    fn resolve_ruleset(
        process: &ProcessMemory,
        rulesets_addr: usize,
        offsets: &RulesetOffsets,
    ) -> Result<usize, MemoryError> {
        let ptr1 = process
            .read_ptr32((rulesets_addr as isize + offsets.ptr_offset) as usize)
            .map_err(|e| MemoryError::ReadFailed(e.to_string()))?;
        if ptr1 == 0 {
            return Err(MemoryError::ReadFailed("Null rulesets ptr".into()));
        }
        let ruleset = process
            .read_ptr32(ptr1 + offsets.ptr_deref)
            .map_err(|e| MemoryError::ReadFailed(e.to_string()))?;
        if ruleset == 0 {
            return Err(MemoryError::ReadFailed("Null ruleset".into()));
        }
        Ok(ruleset)
    }

    fn init_client_process(
        pid: u32,
        slot: i32,
        offsets: &Offsets,
    ) -> Result<TourneyClientProcess, MemoryError> {
        log_debug!("Initializing client slot {} (PID {})", slot, pid);

        let process = ProcessMemory::new(pid).map_err(|e| {
            MemoryError::ReadFailed(format!("Failed to open client {}: {}", slot, e))
        })?;

        log_debug!("Scanning memory for rulesets pattern in client {}", slot);
        let (rulesets_pattern, rulesets_mask) = parse_pattern(&offsets.patterns.rulesets);
        let rulesets_addr = process
            .pattern_scan(&rulesets_pattern, &rulesets_mask)
            .map_err(|e| {
                MemoryError::ReadFailed(format!(
                    "Client {} rulesets pattern scan failed: {}",
                    slot, e
                ))
            })?;

        log_debug!(
            "Client {} rulesets pattern found at 0x{:X}, verifying...",
            slot,
            rulesets_addr
        );
        Self::resolve_ruleset(&process, rulesets_addr, &offsets.ruleset)?;

        log_debug!("Scanning memory for base pattern in client {}", slot);
        let (base_pattern, base_mask) = parse_pattern(&offsets.patterns.base);
        let base_addr = process
            .pattern_scan(&base_pattern, &base_mask)
            .map_err(|e| {
                MemoryError::ReadFailed(format!("Client {} base pattern scan failed: {}", slot, e))
            })?;

        log_debug!("Client {} base pattern found at 0x{:X}", slot, base_addr);

        log_debug!("Client {} initialized successfully", slot);
        Ok(TourneyClientProcess {
            slot,
            pid,
            process,
            rulesets_addr,
            base_addr,
        })
    }

    pub async fn get_data(&self) -> TourneyData {
        self.data.read().await.clone()
    }

    pub async fn get_state(&self) -> TourneyState {
        self.data.read().await.state
    }

    pub async fn poll(&self) -> bool {
        let offsets = match &self.offsets {
            Some(o) => o,
            None => return false,
        };

        let clients = self.clients.read().await;
        if clients.is_empty() {
            return true;
        }

        let mut data = self.data.write().await;
        data.clients.resize(clients.len(), TourneyClient::default());

        if let Some(first_client) = clients.first() {
            Self::read_beatmap_data(first_client, &mut data.beatmap, offsets);
        }

        let num_clients = clients.len() as i32;
        let team_size = num_clients / 2;

        let mut had_failure = false;
        for (i, client) in clients.iter().enumerate() {
            if !Self::read_client_data(client, &mut data.clients[i], offsets) {
                had_failure = true;
            }
            let slot = data.clients[i].slot;
            if team_size > 0 {
                data.clients[i].team = slot / team_size;
            }
        }

        !had_failure
    }

    fn read_beatmap_data(client: &TourneyClientProcess, out: &mut Beatmap, offsets: &Offsets) {
        let p = &client.process;

        let beatmap_ptr_addr = (client.base_addr as isize + offsets.base.beatmap_ptr) as usize;
        let beatmap_ptr = match p.read_ptr32(beatmap_ptr_addr) {
            Ok(ptr) if ptr != 0 => ptr,
            _ => return,
        };

        if let Ok(id) = p.read_i32(beatmap_ptr + offsets.beatmap.map_id) {
            if id > 0 {
                out.id = id;
            }
        }
    }

    // returns false if process is likely dead. returns true with unchanged
    // score/mods when gameplay data unavailable (results screen, lobby, etc).
    fn read_client_data(
        client: &TourneyClientProcess,
        out: &mut TourneyClient,
        offsets: &Offsets,
    ) -> bool {
        let p = &client.process;
        out.slot = client.slot;

        let ruleset_addr = match Self::resolve_ruleset(p, client.rulesets_addr, &offsets.ruleset) {
            Ok(addr) => addr,
            Err(_) => {
                if p.read_ptr32(client.rulesets_addr).is_err() {
                    return false;
                }
                return true;
            }
        };

        let ptr1 = match p.read_ptr32(ruleset_addr + offsets.ruleset.gameplay_ptr) {
            Ok(v) => v,
            Err(_) => return false,
        };

        if ptr1 == 0 {
            return true;
        }

        let base = p
            .read_ptr32(ptr1 + offsets.ruleset.gameplay_base)
            .unwrap_or(0);
        if base == 0 {
            return true;
        }

        if !is_plausible_ptr(base) {
            Self::warn_outdated_offsets(client.slot, ptr1, base);
            return true;
        }

        if !Self::is_gameplay_active(p, base, offsets) {
            return true;
        }

        let raw_score = p
            .read_i32(base + offsets.ruleset.gameplay_score)
            .unwrap_or(0);
        if (0..2_000_000_000).contains(&raw_score) {
            out.score = raw_score;
        }

        let mods_ptr = p.read_ptr32(base + offsets.ruleset.mods_ptr).unwrap_or(0);
        if mods_ptr != 0 {
            let xor1 = p
                .read_i32(mods_ptr + offsets.ruleset.mods_xor1)
                .unwrap_or(0);
            let xor2 = p
                .read_i32(mods_ptr + offsets.ruleset.mods_xor2)
                .unwrap_or(0);
            let mods_raw = (xor1 ^ xor2) as u32;
            if mods_raw < 0x20000 {
                out.mods = parse_mods_bitfield(mods_raw);
            }
        }

        true
    }

    fn is_gameplay_active(p: &ProcessMemory, base: usize, offsets: &Offsets) -> bool {
        let mode = p.read_i32(base + offsets.ruleset.mode).unwrap_or(-1);
        (0..=3).contains(&mode)
    }

    // log values that are likely garbage
    fn warn_outdated_offsets(slot: i32, gameplay_base: usize, score_base: usize) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static THROTTLE: AtomicU32 = AtomicU32::new(0);
        if THROTTLE.fetch_add(1, Ordering::Relaxed) % 300 == 0 {
            log_error!(
                "slot {}: score base resolved to implausible pointer 0x{:X} (via gameplay base 0x{:X})",
                slot,
                score_base,
                gameplay_base
            );
        }
    }
}

fn is_plausible_ptr(addr: usize) -> bool {
    (0x1_0000..=0xFFFF_FFFF).contains(&addr)
}

fn parse_mods_bitfield(mods: u32) -> GameplayMods {
    const NF: u32 = 1;
    const EZ: u32 = 2;
    const TD: u32 = 4;
    const HD: u32 = 8;
    const HR: u32 = 16;
    const SD: u32 = 32;
    const DT: u32 = 64;
    const RX: u32 = 128;
    const HT: u32 = 256;
    const NC: u32 = 512;
    const FL: u32 = 1024;
    const SO: u32 = 4096;
    const AP: u32 = 8192;
    const PF: u32 = 16384;

    if mods == 0 {
        return GameplayMods {
            mods: vec![],
            mods_string: "NM".to_string(),
        };
    }

    let checks: &[(u32, &str)] = &[
        (EZ, "EZ"),
        (NF, "NF"),
        (HT, "HT"),
        (HR, "HR"),
        (SD, "SD"),
        (PF, "PF"),
        (DT, "DT"),
        (NC, "NC"),
        (HD, "HD"),
        (FL, "FL"),
        (RX, "RX"),
        (AP, "AP"),
        (SO, "SO"),
        (TD, "TD"),
    ];

    let mut result = Vec::new();
    for &(flag, acronym) in checks {
        if mods & flag != 0 {
            if flag == NC && mods & DT != 0 {
                continue;
            }
            if flag == PF && mods & SD != 0 {
                continue;
            }
            result.push(ModInfo {
                acronym: acronym.to_string(),
                settings: None,
            });
        }
    }

    let mods_string = if result.is_empty() {
        "NM".to_string()
    } else {
        order_mods(
            &result
                .iter()
                .map(|m| m.acronym.clone())
                .collect::<Vec<_>>()
                .join(""),
        )
    };

    GameplayMods {
        mods: result,
        mods_string,
    }
}
