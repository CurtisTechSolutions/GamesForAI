//! GamesForAI command-line entry point for local play.
use gfa_server::{Config, ServerError};
use std::path::PathBuf;

const USAGE: &str = "Usage: gfa serve [--sqlite PATH] [--port PORT]\n\nStarts a local API on 127.0.0.1 (default port 8080).\nSQLite defaults to ./gfa.sqlite. Port 0 selects an available port.";

fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Config>, String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        None | Some("--help" | "-h") => return Ok(None),
        Some("serve") => {}
        Some(command) => return Err(format!("Unknown command: {command}")),
    }
    let mut config = Config::default();
    let mut sqlite_seen = false;
    let mut port_seen = false;
    while let Some(option) = args.next() {
        match option.as_str() {
            "--help" | "-h" => return Ok(None),
            "--sqlite" if !sqlite_seen => {
                let path = args.next().ok_or("--sqlite requires a path")?;
                if path.is_empty() || path.starts_with("--") { return Err("--sqlite requires a path".into()); }
                config.sqlite = PathBuf::from(path);
                sqlite_seen = true;
            }
            "--port" if !port_seen => {
                config.port = args.next().ok_or("--port requires a number")?.parse().map_err(|_| "Port must be an integer from 0 to 65535")?;
                port_seen = true;
            }
            _ => return Err(format!("Unknown or repeated option: {option}")),
        }
    }
    Ok(Some(config))
}

async fn shutdown() -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            signal = tokio::signal::ctrl_c() => signal?,
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), ServerError> {
    match parse(std::env::args().skip(1)) {
        Ok(Some(config)) => gfa_server::serve(config, async {
            if let Err(error) = shutdown().await { eprintln!("Shutdown signal handler failed: {error}"); }
        }).await,
        Ok(None) => { println!("{USAGE}"); Ok(()) }
        Err(error) => { eprintln!("{error}\n\n{USAGE}"); std::process::exit(2); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(args: &[&str]) -> Result<Option<Config>, String> {
        parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn parses_local_configuration_and_help() -> Result<(), String> {
        assert_eq!(options(&["serve"])?, Some(Config::default()));
        assert_eq!(options(&["serve", "--sqlite", "matches.sqlite", "--port", "0"])?, Some(Config { sqlite: "matches.sqlite".into(), port: 0 }));
        assert!(options(&[])?.is_none());
        assert!(options(&["--help"])?.is_none());
        Ok(())
    }

    #[test]
    fn rejects_invalid_or_ambiguous_options() {
        for args in [
            vec!["unknown"], vec!["serve", "--bind", "0.0.0.0"],
            vec!["serve", "--port", "65536"], vec!["serve", "--port", "-1"],
            vec!["serve", "--sqlite"], vec!["serve", "--sqlite", "--port"],
            vec!["serve", "--port", "80", "--port", "81"],
        ] { assert!(options(&args).is_err(), "{args:?}"); }
    }
}
