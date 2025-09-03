use std::fs::File;
use std::os::unix::io::AsRawFd;

pub fn redirect_stderr(path: &str) -> std::io::Result<()> {
    let file = File::create(path)?;
    let fd = file.as_raw_fd();
    // dup2 replaces STDERR_FILENO with file descriptor
    unsafe { libc::dup2(fd, libc::STDERR_FILENO); }
    Ok(())
}
