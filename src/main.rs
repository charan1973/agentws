use agentws::cli;

fn main() -> anyhow::Result<()> {
    // Restore default SIGPIPE handling so piping into `head`/`grep` exits
    // quietly instead of panicking on a broken pipe.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    cli::run()
}
