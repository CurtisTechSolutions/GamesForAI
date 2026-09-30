//! GamesForAI command-line entry point for local play.
use gfa_server::{Config, Database, ServerError};
use std::path::PathBuf;

const USAGE: &str = "Usage: gfa serve [--sqlite PATH | --postgres-env VARIABLE] [--port PORT] [--stockfish PATH]\n\nStarts a local API on 127.0.0.1 (default port 8080).\nSQLite defaults to ./gfa.sqlite. Port 0 selects an available port.\nPostgreSQL requires the postgres build feature and reads its URL from VARIABLE.\nStockfish requires an absolute binary path, Linux, bubblewrap and working user namespaces.";

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
    let mut stockfish_seen = false;
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
            "--stockfish" if !stockfish_seen => {
                let path = PathBuf::from(args.next().ok_or("--stockfish requires an absolute binary path")?);
                if !path.is_absolute() {
                    return Err("--stockfish requires an absolute binary path".into());
                }
                config.stockfish = Some(gfa_server::StockfishConfig::linux(path));
                stockfish_seen = true;
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

#[tokio::main]
async fn main() -> Result<(), ServerError> {
    match parse(std::env::args().skip(1)) {
        Ok(Some(config)) => {
            gfa_server::serve(config, async {
                if let Err(error) = shutdown().await {
                    eprintln!("Shutdown signal handler failed: {error}");
                }
            })
            .await
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
            })
        );
        let configured = options(&["serve", "--stockfish", "/usr/games/stockfish"])?.ok_or("missing config")?;
        assert_eq!(configured.stockfish, Some(gfa_server::StockfishConfig::linux("/usr/games/stockfish")));
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
    fn rejects_invalid_or_ambiguous_options() {
        for args in [
            vec!["unknown"],
            vec!["serve", "--bind", "0.0.0.0"],
            vec!["serve", "--port", "65536"],
            vec!["serve", "--port", "-1"],
            vec!["serve", "--stockfish"],
            vec!["serve", "--stockfish", "relative"],
            vec!["serve", "--stockfish", "/usr/games/stockfish", "--stockfish", "/usr/games/stockfish"],
            vec!["serve", "--sqlite"],
            vec!["serve", "--sqlite", "--port"],
            vec!["serve", "--port", "80", "--port", "81"],
        ] {
            assert!(options(&args).is_err(), "{args:?}");
        }
    }
}
