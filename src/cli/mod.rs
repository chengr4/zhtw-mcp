// Command-line front end: everything that exists to serve the binary rather
// than the library.

pub(crate) mod args;
pub(crate) mod discover;
pub(crate) mod hook;
pub(crate) mod lint;
pub(crate) mod render;

/// The `--help` texts, extracted from the `<!-- cli:<name> -->` blocks in
/// docs/cli.md by build.rs.  The docs are the single source of truth, so the
/// binary cannot print help that disagrees with them; tests/cli-help.rs
/// checks the setup host list against `setup::ALL_HOSTS`.
pub(crate) mod help {
    include!(concat!(env!("OUT_DIR"), "/cli_help.rs"));
}
