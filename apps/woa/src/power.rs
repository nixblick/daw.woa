//! Windows: WOA vom Energiesparen ausnehmen (0.42.1).
//!
//! Windows 11 drosselt Prozesse, deren Fenster nicht vorn ist (EcoQoS, gröbere Zeitgeber).
//! Beim Wechsel zum Explorer knisterte so der Ton. Hier sagt WOA beim Start: nicht drosseln,
//! Zeitgeber-Auflösung behalten. Die Audio-Threads meldet cpal zusätzlich als „Pro Audio“
//! (MMCSS, Feature `realtime`) an. Ohne neue Abhängigkeit, nur ein Aufruf in kernel32.

/// Schaltet die Drosselung für diesen Prozess ab. Fehler nur ins Log, der Start geht weiter.
#[cfg(windows)]
pub fn no_throttling() {
    use std::ffi::c_void;

    #[repr(C)]
    struct ProcessPowerThrottlingState {
        version: u32,
        control_mask: u32,
        state_mask: u32,
    }
    const PROCESS_POWER_THROTTLING: i32 = 4;
    const CURRENT_VERSION: u32 = 1;
    const EXECUTION_SPEED: u32 = 0x1;
    const IGNORE_TIMER_RESOLUTION: u32 = 0x4;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn SetProcessInformation(
            process: *mut c_void,
            class: i32,
            info: *const c_void,
            size: u32,
        ) -> i32;
    }

    // Maske gesetzt, Zustand 0: diese Drosselungen sind ausdrücklich aus.
    let state = ProcessPowerThrottlingState {
        version: CURRENT_VERSION,
        control_mask: EXECUTION_SPEED | IGNORE_TIMER_RESOLUTION,
        state_mask: 0,
    };
    // SAFETY: Pseudo-Handle des eigenen Prozesses; die Struktur lebt bis zum Ende des Aufrufs
    // und hat das von Windows erwartete Layout (drei u32).
    let ok = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            PROCESS_POWER_THROTTLING,
            (&raw const state).cast(),
            std::mem::size_of::<ProcessPowerThrottlingState>() as u32,
        )
    };
    if ok == 0 {
        tracing::warn!(
            error = %std::io::Error::last_os_error(),
            "power throttling not disabled"
        );
    } else {
        tracing::info!("power throttling disabled");
    }
}

#[cfg(not(windows))]
pub fn no_throttling() {}
