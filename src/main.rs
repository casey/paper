//! Implements the entry point for the `paper` binary.
use {
    // `app_from_crate` requires using all the macros that it calls.
    clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg},
    core::convert::TryFrom,
    paper::{Arguments, Failure, Paper},
};

fn main() -> Result<(), Failure> {
    // Forces compiler to rebuild when Cargo.toml file is changed, needed for app_from_crate.
    let _ = include_str!("../Cargo.toml");
    let args = Arguments::try_from(
        app_from_crate!()
            .arg(Arg::with_name("file").help("the file to be viewed"))
            .get_matches(),
    )?;

    Paper::new(args)?.run()?;
    Ok(())
}
