//! A development-only entry point using the same pinned engine as Book tests.
use std::{
    io::{self, Read},
    path::Path,
    process::ExitCode,
};

fn main() -> anyhow::Result<ExitCode> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("contract") => {
            print!("{}", anchor_book::contract::text());
            Ok(ExitCode::SUCCESS)
        }
        Some("check") => {
            let path = if args.len() == 3 && args[1] == "--config" {
                args[2].as_str()
            } else {
                "anchorbook.toml"
            };
            let report =
                anchor_book::checking::check(Path::new(path)).unwrap_or_else(|e| e.into_report());
            print!("{}", anchor_book::adapters::check::format_report(&report));
            Ok(ExitCode::from(report.outcome().exit_code()))
        }
        Some("md") if args.get(1).map(String::as_str) == Some("supports") => {
            Ok(if args.get(2).map(String::as_str) == Some("html") {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Some("md") => {
            let mut request = String::new();
            io::stdin().read_to_string(&mut request)?;
            print!("{}", anchor_book::adapters::mdbook::preprocess(&request)?);
            Ok(ExitCode::SUCCESS)
        }
        _ => anyhow::bail!(
            "Usage: cargo run --example anchorbook -- contract | check [--config PATH] | md [supports html]"
        ),
    }
}
