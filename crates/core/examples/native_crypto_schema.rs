//! Generate collection/config schemas using the workspace lockfile.
use slumber_config::Config;
use slumber_core::collection::Collection;
use std::{fs, path::Path, process::ExitCode};
fn main() -> ExitCode {
    let check = std::env::args().any(|arg| arg == "--check");
    for (name, schema) in [
        ("collection", schemars::schema_for!(Collection)),
        ("config", schemars::schema_for!(Config)),
    ] {
        let text =
            serde_json::to_string_pretty(&schema).expect("schema is JSON");
        let path = Path::new("schemas").join(format!("{name}.json"));
        if check {
            if fs::read_to_string(&path).ok().as_deref() != Some(text.as_str())
            {
                eprintln!("{} is stale", path.display());
                return ExitCode::FAILURE;
            }
        } else {
            fs::write(&path, text).expect("write schema");
        }
    }
    ExitCode::SUCCESS
}
