use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

pub fn redirect_stderr(path: &str) -> std::io::Result<()> {
    let file = File::create(path)?;
    let fd = file.as_raw_fd();
    // dup2 replaces STDERR_FILENO with file descriptor
    unsafe { libc::dup2(fd, libc::STDERR_FILENO); }
    Ok(())
}

pub fn get_abs_path(rel: &str) -> PathBuf {
    let src = std::path::Path::new(file!());
    src.join(rel)
}
