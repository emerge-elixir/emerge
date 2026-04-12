#[cfg(all(feature = "macos", target_os = "macos"))]
mod app {
    use std::{
        thread,
        time::{Duration, Instant},
    };

    use objc2::{MainThreadMarker, MainThreadOnly, rc::autoreleasepool};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEventMask, NSWindow,
        NSWindowStyleMask,
    };
    use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString};

    pub fn run() -> Result<(), String> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "macos_smoke must run on the macOS process main thread".to_string())?;

        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        app.finishLaunching();

        run_window_cycle(&app, mtm, "EmergeSkia macOS smoke 1", 640, 420, 1_000)?;
        run_window_cycle(&app, mtm, "EmergeSkia macOS smoke 2", 700, 460, 1_000)?;
        Ok(())
    }

    fn run_window_cycle(
        app: &NSApplication,
        mtm: MainThreadMarker,
        title: &str,
        width: u32,
        height: u32,
        duration_ms: u64,
    ) -> Result<(), String> {
        let window = create_window(app, mtm, title, width, height);
        let deadline = Instant::now() + Duration::from_millis(duration_ms);
        let distant_past = NSDate::distantPast();

        while window.isVisible() && Instant::now() < deadline {
            autoreleasepool(|_| {
                while let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    Some(&distant_past),
                    unsafe { NSDefaultRunLoopMode },
                    true,
                ) {
                    app.sendEvent(&event);
                }
            });

            app.updateWindows();
            thread::sleep(Duration::from_millis(10));
        }

        window.close();

        let close_deadline = Instant::now() + Duration::from_millis(500);
        while window.isVisible() && Instant::now() < close_deadline {
            autoreleasepool(|_| {
                while let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    Some(&distant_past),
                    unsafe { NSDefaultRunLoopMode },
                    true,
                ) {
                    app.sendEvent(&event);
                }
            });

            app.updateWindows();
            thread::sleep(Duration::from_millis(10));
        }

        if window.isVisible() {
            Err(format!("window did not close cleanly for cycle: {title}"))
        } else {
            Ok(())
        }
    }

    fn create_window(
        app: &NSApplication,
        mtm: MainThreadMarker,
        title: &str,
        width: u32,
        height: u32,
    ) -> objc2::rc::Retained<NSWindow> {
        let frame = NSRect::new(
            NSPoint::new(120.0, 120.0),
            NSSize::new(width as f64, height as f64),
        );
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };

        let title = NSString::from_str(title);
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        window.setTitle(&title);
        window.center();
        window.makeKeyAndOrderFront(None);
        app.activate();
        window
    }
}

#[cfg(all(feature = "macos", target_os = "macos"))]
fn main() {
    if let Err(reason) = app::run() {
        eprintln!("macOS smoke failed: {reason}");
        std::process::exit(1);
    }

    println!("macOS smoke passed");
}

#[cfg(not(all(feature = "macos", target_os = "macos")))]
fn main() {
    eprintln!("macos_smoke can only run on macOS");
    std::process::exit(1);
}
