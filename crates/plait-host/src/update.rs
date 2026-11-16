//! `plait-host update`: installs the newest signed release's host on this
//! machine, for hosts that run without the desktop app.

use plait_core::update::{self, install};
use std::io;

pub fn run(args: Vec<String>) -> io::Result<()> {
    let check_only = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] => false,
        ["--check"] => true,
        _ => return Err(io::Error::other("usage: plait-host update [--check]")),
    };
    if !cfg!(target_os = "macos") {
        return Err(io::Error::other(
            "host updates currently install only on macOS",
        ));
    }
    let current = update::current_version();
    let running = current
        .as_ref()
        .map_or("a source build".to_owned(), |version| {
            format!("Plait {version}")
        });
    let Some(offer) = update::check(current.as_ref()).map_err(io::Error::other)? else {
        println!("No newer signed release is published; this host runs {running}.");
        return Ok(());
    };
    println!("Plait {} is available (running {running}).", offer.version);
    if check_only {
        println!("Release notes: {}", offer.notes_url());
        return Ok(());
    }
    let home = crate::service::user_home()?;
    let program = install::apply_host_update(&offer, &home).map_err(io::Error::other)?;
    println!("Host {} is active: {}", offer.version, program.display());
    Ok(())
}
