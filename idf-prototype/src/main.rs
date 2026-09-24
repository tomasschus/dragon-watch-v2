mod power;

use std::time::Duration;

fn main() {
    esp_idf_sys::link_patches();
    power::configure().expect("ESP-IDF PM configuration failed");
    let performance = power::PerformanceLock::new().expect("CPU lock creation failed");
    println!("ESP-IDF prototype: 80 MHz idle, 160 MHz while an app holds the lock");

    loop {
        {
            let _app = performance.enter().expect("CPU lock acquire failed");
            println!("App open: performance lock acquired");
            std::thread::sleep(Duration::from_secs(2));
        }
        println!("App closed: performance lock released");
        std::thread::sleep(Duration::from_secs(2));
    }
}
