//! Repository checks, run with cargo xtask check-deps.
use serde_json::Value;
use std::{collections::BTreeMap, error::Error, process::Command};

fn allowed(from: &str, to: &str, dev: bool) -> bool {
    if !to.starts_with("gfa-") {
        return true;
    }
    if dev && to == "gfa-testkit" {
        return true;
    }
    if from == "gfa-core" {
        return false;
    }
    if from == "gfa-testkit" {
        return to == "gfa-core";
    }
    if from.starts_with("gfa-game-") {
        return to == "gfa-core";
    }
    if from == "gfa-games" {
        return to == "gfa-core" || to.starts_with("gfa-game-");
    }
    let players = ["gfa-opponents", "gfa-engine-uci", "gfa-engine-gtp", "gfa-llm"];
    if players.contains(&from) {
        return to == "gfa-core";
    }
    if from == "gfa-api-types" {
        return to == "gfa-core";
    }
    if from == "gfa-service" {
        return to == "gfa-core" || to == "gfa-api-types" || players.contains(&to);
    }
    if from == "gfa-bench" {
        return to == "gfa-core" || to == "gfa-api-types" || to == "gfa-service" || players.contains(&to);
    }
    if ["gfa-store", "gfa-http", "gfa-mcp"].contains(&from) {
        return to == "gfa-core" || to == "gfa-api-types" || to == "gfa-service" || to == "gfa-bench";
    }
    ["gfa-server", "gfa-cli", "gfa-py"].contains(&from)
}

fn forbidden_external(layer: &str, name: &str) -> bool {
    if layer == "gfa-core" || layer.starts_with("gfa-game-") {
        return ["tokio", "async-std", "reqwest", "sqlx", "axum", "rmcp", "pyo3", "tracing", "log"].contains(&name);
    }
    if ["gfa-service", "gfa-bench", "gfa-api-types"].contains(&layer) {
        return ["axum", "sqlx", "rmcp", "pyo3", "reqwest"].contains(&name);
    }
    false
}

fn check_deps(metadata: &Value) -> Result<(), Box<dyn Error>> {
    let members = metadata["workspace_members"].as_array().ok_or("missing members")?;
    let packages = metadata["packages"].as_array().ok_or("missing packages")?;
    let mut violations = BTreeMap::new();
    for package in packages.iter().filter(|p| members.contains(&p["id"])) {
        let from = package["name"].as_str().ok_or("missing package name")?;
        for dep in package["dependencies"].as_array().ok_or("missing dependencies")? {
            let to = dep["name"].as_str().ok_or("missing dependency name")?;
            let dev = dep["kind"] == "dev";
            if !allowed(from, to, dev) || (!dev && forbidden_external(from, to)) {
                violations.insert(format!("{from} -> {to}"), "violates PRD §5.2");
            }
        }
    }
    if violations.is_empty() {
        println!("Dependency direction: all workspace crates pass.");
        Ok(())
    } else {
        Err(format!("{violations:#?}").into())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().nth(1).as_deref() != Some("check-deps") {
        return Err("usage: cargo xtask check-deps".into());
    }
    let output = Command::new("cargo").args(["metadata", "--format-version", "1", "--no-deps", "--all-features"]).output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string().into());
    }
    check_deps(&serde_json::from_slice(&output.stdout)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denies_cross_layer_edges() {
        for (from, to) in [
            ("gfa-service", "gfa-game-tictactoe"),
            ("gfa-opponents", "gfa-service"),
            ("gfa-http", "gfa-mcp"),
            ("gfa-store", "gfa-http"),
            ("gfa-game-tictactoe", "gfa-games"),
            ("gfa-core", "gfa-service"),
        ] {
            assert!(!allowed(from, to, false), "{from} -> {to}");
        }
        assert!(forbidden_external("gfa-service", "sqlx"));
        assert!(forbidden_external("gfa-game-chess", "tokio"));
        assert!(allowed("gfa-games", "gfa-game-chess", false));
        assert!(allowed("gfa-game-chess", "gfa-testkit", true));
    }
}
