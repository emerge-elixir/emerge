#[cfg(all(feature = "macos", target_os = "macos"))]
mod app {
    use std::{process, thread};

    use objc2::{MainThreadMarker, rc::autoreleasepool};
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_metal::{MTLCopyAllDevices, MTLCreateSystemDefaultDevice};

    pub fn run() {
        autoreleasepool(|_| {
            print_probe("before_appkit");

            if let Some(mtm) = MainThreadMarker::new() {
                let app = NSApplication::sharedApplication(mtm);
                let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
                app.finishLaunching();
                print_probe("after_appkit");
            } else {
                println!("phase=after_appkit unavailable reason=not_main_thread");
            }
        });
    }

    fn print_probe(phase: &str) {
        let default_device = MTLCreateSystemDefaultDevice();
        let all_devices = MTLCopyAllDevices();

        println!(
            "phase={phase} pid={} ppid={} rust_thread={:?} pthread_main={} objc_main={} default_device_present={} all_devices_count={}",
            process::id(),
            parent_pid(),
            thread::current().id(),
            unsafe { libc::pthread_main_np() == 1 },
            MainThreadMarker::new().is_some(),
            default_device.is_some(),
            all_devices.len(),
        );
    }

    fn parent_pid() -> i32 {
        unsafe { libc::getppid() }
    }
}

#[cfg(all(feature = "macos", target_os = "macos"))]
fn main() {
    app::run();
}

#[cfg(not(all(feature = "macos", target_os = "macos")))]
fn main() {
    eprintln!("metal_probe can only run on macOS");
    std::process::exit(1);
}
