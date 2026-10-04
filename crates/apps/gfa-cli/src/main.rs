//! GamesForAI command-line entry point for local play.
use gfa_server::{Config, Database, ServerError};
use std::path::PathBuf;

const USAGE: &str = "Usage: gfa mcp [--sqlite PATH | --postgres-env VARIABLE] [--seat SEAT | --spectator] [--stockfish PATH]\n       gfa serve [--sqlite PATH | --postgres-env VARIABLE] [--port PORT] [--web-dir DIRECTORY] [--stockfish PATH] [--mcp-seat SEAT | --mcp-spectator]\n       gfa tournament --game GAME --agents FILES [--opponents LADDER] [--games COUNT] [--report PATH]\n\nmcp serves a trusted local client over stdin/stdout; seat defaults to 0. No TCP port is opened.\nserve starts a local API on 127.0.0.1 (default port 8080). Add --web-dir to serve a browser build.\nWithout --web-dir, / redirects to /docs/. make serve builds and serves the browser app.\nHTTP /mcp defaults to seat 0; use --mcp-seat or --mcp-spectator to change it.\ntournament uses gamesforai[tournaments] in GFA_PYTHON (default python3; python on Windows).\nSQLite defaults to ./gfa.sqlite. Port 0 selects an available port.\nPostgreSQL requires the postgres build feature and reads its URL from VARIABLE.\nStockfish requires an absolute binary path, Linux, bubblewrap and working user namespaces.";

fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Config>, String> {
    parse_with_env(args, |name| std::env::var(name).ok())
}

fn parse_with_env(
    args: impl IntoIterator<Item = String>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<Option<Config>, String> {
    // Keep the same parser signature in SQLite-only builds.
    let _ = &env;
    let mut args = args.into_iter();
    match args.next().as_deref() {
        None | Some("--help" | "-h") => return Ok(None),
        Some("serve") => {}
        Some(command) => return Err(format!("Unknown command: {command}")),
    }
    let mut config = Config::default();
    let mut database_seen = false;
    let mut port_seen = false;
    let mut web_seen = false;
    let mut stockfish_seen = false;
    let mut mcp_viewer_seen = false;
    while let Some(option) = args.next() {
        match option.as_str() {
            "--help" | "-h" => return Ok(None),
            "--sqlite" if !database_seen => {
                let path = args.next().ok_or("--sqlite requires a path")?;
                if path.is_empty() || path.starts_with("--") {
                    return Err("--sqlite requires a path".into());
                }
                config.database = Database::Sqlite(PathBuf::from(path));
                database_seen = true;
            }
            #[cfg(feature = "postgres")]
            "--postgres-env" if !database_seen => {
                let name = args
                    .next()
                    .ok_or("--postgres-env requires an environment variable name")?;
                if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    return Err("--postgres-env requires an environment variable name".into());
                }
                let url = env(&name)
                    .ok_or("PostgreSQL environment variable is missing or not Unicode")?;
                if !(url.starts_with("postgres://") || url.starts_with("postgresql://")) {
                    return Err(
                        "PostgreSQL environment variable must contain a PostgreSQL URL".into(),
                    );
                }
                config.database = Database::Postgres(url);
                database_seen = true;
            }
            #[cfg(not(feature = "postgres"))]
            "--postgres-env" => {
                return Err("PostgreSQL requires a build with --features postgres".into())
            }
            "--mcp-seat" if !mcp_viewer_seen => {
                config.mcp_seat = Some(
                    args.next()
                        .ok_or("--mcp-seat requires a seat")?
                        .parse()
                        .map_err(|_| "MCP seat must be an integer from 0 to 255")?,
                );
                mcp_viewer_seen = true;
            }
            "--mcp-spectator" if !mcp_viewer_seen => {
                config.mcp_seat = None;
                mcp_viewer_seen = true;
            }
            "--stockfish" if !stockfish_seen => {
                let path = PathBuf::from(
                    args.next()
                        .ok_or("--stockfish requires an absolute binary path")?,
                );
                if !path.is_absolute() {
                    return Err("--stockfish requires an absolute binary path".into());
                }
                config.stockfish = Some(gfa_server::StockfishConfig::linux(path));
                stockfish_seen = true;
            }
            "--web-dir" if !web_seen => {
                let path = args.next().ok_or("--web-dir requires a build directory")?;
                if path.is_empty() || path.starts_with("--") {
                    return Err("--web-dir requires a build directory".into());
                }
                config.web_dir = Some(PathBuf::from(path));
                web_seen = true;
            }
            "--port" if !port_seen => {
                config.port = args
                    .next()
                    .ok_or("--port requires a number")?
                    .parse()
                    .map_err(|_| "Port must be an integer from 0 to 65535")?;
                port_seen = true;
            }
            _ => return Err(format!("Unknown or repeated option: {option}")),
        }
    }
    Ok(Some(config))
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Serve(Config),
    Mcp { config: Config, seat: Option<u8> },
}
fn command(args: impl IntoIterator<Item = String>) -> Result<Option<Command>, String> {
    let args = args.into_iter().collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("mcp") {
        return parse(args).map(|config| config.map(Command::Serve));
    }
    let mut seat = Some(0);
    let mut viewer_seen = false;
    let mut forwarded = vec!["serve".to_string()];
    let mut arguments = args.into_iter().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--seat" if !viewer_seen => {
                seat = Some(
                    arguments
                        .next()
                        .ok_or("--seat requires a zero-based seat")?
                        .parse()
                        .map_err(|_| "Seat must be an integer from 0 to 255")?,
                );
                viewer_seen = true;
            }
            "--spectator" if !viewer_seen => {
                seat = None;
                viewer_seen = true;
            }
            "--seat" | "--spectator" => return Err("Choose --seat or --spectator once".into()),
            "--web-dir" => return Err("--web-dir is only supported by gfa serve".into()),
            "--mcp-seat" | "--mcp-spectator" => {
                return Err("Use --seat or --spectator for mcp stdio".into())
            }
            "--port" => return Err("mcp uses stdin/stdout and does not accept --port".into()),
            // Preserve values even when a filesystem path equals another option.
            "--sqlite" | "--postgres-env" | "--stockfish" => {
                forwarded.push(argument);
                forwarded.push(
                    arguments
                        .next()
                        .ok_or("Storage and engine options require a value")?,
                );
            }
            _ => forwarded.push(argument),
        }
    }
    parse(forwarded).map(|config| config.map(|config| Command::Mcp { config, seat }))
}

async fn shutdown() -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            signal = tokio::signal::ctrl_c() => signal?,
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = shutdown().await {
        eprintln!("Shutdown signal handler failed: {error}");
    }
}

async fn run() -> Result<(), ServerError> {
    match command(std::env::args().skip(1)) {
        Ok(Some(Command::Serve(config))) => gfa_server::serve(config, shutdown_signal()).await,
        Ok(Some(Command::Mcp { config, seat })) => {
            gfa_server::serve_stdio(config, seat, shutdown_signal()).await
        }
        Ok(None) => {
            println!("{USAGE}");
            Ok(())
        }
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            std::process::exit(2);
        }
    }
}
fn tournament() -> ! {
    let python = std::env::var_os("GFA_PYTHON")
        .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.into());
    let result = std::process::Command::new(python)
        .args(["-m", "gamesforai.tournament_cli"])
        .args(std::env::args_os().skip(2))
        .status();
    let code = match result {
        Ok(status) => status.code().unwrap_or(1),
        Err(_) => {
            eprintln!("Cannot start tournament Python. Install gamesforai[tournaments] and set GFA_PYTHON to its Python executable.");
            1
        }
    };
    std::process::exit(code);
}

fn main() -> Result<(), ServerError> {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "tournament")
    {
        tournament();
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(run());
    // Tokio's stdin reader uses an uncancellable blocking read. A launcher may
    // leave stdin open when sending SIGTERM; do not wait forever for that reader.
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    result
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
        assert_eq!(
            options(&["serve", "--sqlite", "matches.sqlite", "--port", "0"])?,
            Some(Config {
                database: Database::Sqlite("matches.sqlite".into()),
                port: 0,
                stockfish: None,
                mcp_seat: Some(0),
                web_dir: None,
            })
        );
        let configured =
            options(&["serve", "--stockfish", "/usr/games/stockfish"])?.ok_or("missing config")?;
        assert_eq!(
            configured.stockfish,
            Some(gfa_server::StockfishConfig::linux("/usr/games/stockfish"))
        );
        assert!(options(&[])?.is_none());
        assert!(options(&["--help"])?.is_none());
        Ok(())
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn reads_postgres_from_environment_without_exposing_the_url() -> Result<(), String> {
        let url = "postgres://alice:secret@localhost/gfa?sslmode=verify-full";
        let parsed = parse_with_env(
            ["serve", "--postgres-env", "GFA_DATABASE_URL"].map(str::to_owned),
            |name| (name == "GFA_DATABASE_URL").then(|| url.to_owned()),
        )?
        .ok_or("missing config")?;
        assert_eq!(parsed.database, Database::Postgres(url.to_owned()));
        assert!(!format!("{parsed:?}").contains("secret"));
        for options in [
            vec!["serve", "--postgres-env"],
            vec!["serve", "--postgres-env", "missing"],
            vec!["serve", "--postgres-env", "--port"],
            vec![
                "serve",
                "--sqlite",
                "x.sqlite",
                "--postgres-env",
                "GFA_DATABASE_URL",
            ],
            vec![
                "serve",
                "--postgres-env",
                "GFA_DATABASE_URL",
                "--sqlite",
                "x.sqlite",
            ],
            vec![
                "serve",
                "--postgres-env",
                "GFA_DATABASE_URL",
                "--postgres-env",
                "GFA_DATABASE_URL",
            ],
        ] {
            let result = parse_with_env(options.iter().map(|s| (*s).to_owned()), |name| {
                (name == "GFA_DATABASE_URL").then(|| url.to_owned())
            });
            assert!(result.is_err(), "{options:?}");
        }
        let error = parse_with_env(
            ["serve", "--postgres-env", "GFA_DATABASE_URL"].map(str::to_owned),
            |_| Some("invalid-secret-url".into()),
        )
        .err()
        .ok_or("invalid URL accepted")?;
        assert!(!error.contains("invalid-secret-url"));
        Ok(())
    }

    #[cfg(not(feature = "postgres"))]
    #[test]
    fn explains_missing_postgres_feature() {
        assert!(options(&["serve", "--postgres-env", "GFA_DATABASE_URL"])
            .err()
            .is_some_and(|error| error.contains("--features postgres")));
    }

    #[test]
    fn browser_directory_is_explicit_and_only_supported_for_http() -> Result<(), String> {
        let configured = options(&["serve", "--web-dir", "web build"])?.ok_or("missing config")?;
        assert_eq!(configured.web_dir, Some(PathBuf::from("web build")));
        for args in [
            vec!["serve", "--web-dir"],
            vec!["serve", "--web-dir", ""],
            vec!["serve", "--web-dir", "--port"],
            vec!["serve", "--web-dir", "one", "--web-dir", "two"],
        ] {
            assert!(options(&args).is_err(), "{args:?}");
        }
        assert!(command(["mcp", "--web-dir", "web"].map(str::to_owned)).is_err());
        Ok(())
    }

    #[test]
    fn rejects_invalid_or_ambiguous_options() {
        for args in [
            vec!["unknown"],
            vec!["serve", "--bind", "0.0.0.0"],
            vec!["serve", "--port", "65536"],
            vec!["serve", "--port", "-1"],
            vec!["serve", "--stockfish"],
            vec!["serve", "--stockfish", "relative"],
            vec![
                "serve",
                "--stockfish",
                "/usr/games/stockfish",
                "--stockfish",
                "/usr/games/stockfish",
            ],
            vec!["serve", "--sqlite"],
            vec!["serve", "--sqlite", "--port"],
            vec!["serve", "--port", "80", "--port", "81"],
        ] {
            assert!(options(&args).is_err(), "{args:?}");
        }
    }
}

#[cfg(test)]
mod mcp_command_tests {
    use super::*;
    fn parsed(args: &[&str]) -> Result<Option<Command>, String> {
        command(args.iter().map(|arg| (*arg).to_string()))
    }
    #[test]
    fn parses_stdio_and_rejects_ambiguous_viewers() -> Result<(), String> {
        assert_eq!(
            parsed(&["mcp"])?,
            Some(Command::Mcp {
                config: Config::default(),
                seat: Some(0)
            })
        );
        assert_eq!(
            parsed(&["mcp", "--seat", "1"])?,
            Some(Command::Mcp {
                config: Config::default(),
                seat: Some(1)
            })
        );
        assert_eq!(
            parsed(&["mcp", "--spectator"])?,
            Some(Command::Mcp {
                config: Config::default(),
                seat: None
            })
        );
        assert!(parsed(&["mcp", "--help"])?.is_none());
        for args in [
            vec!["mcp", "--seat"],
            vec!["mcp", "--seat", "256"],
            vec!["mcp", "--seat", "-1"],
            vec!["mcp", "--seat", "0", "--spectator"],
            vec!["mcp", "--spectator", "--seat", "0"],
            vec!["mcp", "--spectator", "--spectator"],
            vec!["mcp", "--port", "0"],
            vec!["serve", "--seat", "1"],
            vec!["mcp", "--sqlite"],
        ] {
            assert!(parsed(&args).is_err(), "{args:?}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod http_mcp_options_tests {
    use super::*;
    #[test]
    fn validates_http_mcp_viewer_options() -> Result<(), String> {
        let config = parse(["serve", "--mcp-seat", "1"].map(str::to_owned))?.ok_or("config")?;
        assert_eq!(config.mcp_seat, Some(1));
        assert_eq!(
            parse(["serve", "--mcp-spectator"].map(str::to_owned))?
                .ok_or("config")?
                .mcp_seat,
            None
        );
        for args in [
            vec!["serve", "--mcp-seat"],
            vec!["serve", "--mcp-seat", "256"],
            vec!["serve", "--mcp-seat", "0", "--mcp-spectator"],
            vec!["serve", "--mcp-spectator", "--mcp-seat", "0"],
            vec!["mcp", "--mcp-seat", "1"],
            vec!["mcp", "--mcp-spectator"],
        ] {
            assert!(command(args.into_iter().map(str::to_owned)).is_err());
        }
        Ok(())
    }
}
