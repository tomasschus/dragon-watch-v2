//! Prueba aislada de la gestión de frecuencia de ESP-IDF.
//! Una app toma el lock al abrir y lo libera al cerrar.

use esp_idf_sys::{
    esp_err_t, esp_pm_config_t, esp_pm_configure, esp_pm_lock_acquire,
    esp_pm_lock_create, esp_pm_lock_delete, esp_pm_lock_handle_t,
    esp_pm_lock_release, ESP_OK, esp_pm_lock_type_t_ESP_PM_CPU_FREQ_MAX,
};
use std::ffi::c_void;
use std::ptr;

fn check(result: esp_err_t) -> Result<(), esp_err_t> {
    if result == ESP_OK { Ok(()) } else { Err(result) }
}

pub fn configure() -> Result<(), esp_err_t> {
    let config = esp_pm_config_t {
        min_freq_mhz: 80,
        max_freq_mhz: 160,
        light_sleep_enable: false,
    };
    check(unsafe { esp_pm_configure((&config as *const esp_pm_config_t).cast::<c_void>()) })
}

pub struct PerformanceLock {
    handle: esp_pm_lock_handle_t,
}

impl PerformanceLock {
    pub fn new() -> Result<Self, esp_err_t> {
        let mut handle = ptr::null_mut();
        check(unsafe {
            esp_pm_lock_create(
                esp_pm_lock_type_t_ESP_PM_CPU_FREQ_MAX,
                0,
                c"watch-app".as_ptr(),
                &mut handle,
            )
        })?;
        Ok(Self { handle })
    }

    pub fn enter(&self) -> Result<PerformanceGuard<'_>, esp_err_t> {
        check(unsafe { esp_pm_lock_acquire(self.handle) })?;
        Ok(PerformanceGuard { lock: self })
    }
}

impl Drop for PerformanceLock {
    fn drop(&mut self) {
        unsafe { esp_pm_lock_delete(self.handle) };
    }
}

pub struct PerformanceGuard<'a> {
    lock: &'a PerformanceLock,
}

impl Drop for PerformanceGuard<'_> {
    fn drop(&mut self) {
        let result = unsafe { esp_pm_lock_release(self.lock.handle) };
        debug_assert_eq!(result, ESP_OK);
    }
}
