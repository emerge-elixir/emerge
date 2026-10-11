//! Hardware-independent smoke test; runnable on an ordinary Nerves image.
use std::os::fd::AsRawFd;
fn main() -> std::io::Result<()> {
    let mut context = emerge_input::Context::new()?;
    assert!(
        context.validate_bundled_quirks(),
        "embedded quirks must parse"
    );
    context.dispatch()?;
    println!("bundled libinput ready, fd={}", context.as_raw_fd());
    Ok(())
}
