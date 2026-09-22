//! Wasm Hands host for Helix.
//!
//! Runs WebAssembly modules under Wasmtime with **deny-by-default WASI**:
//!
//! - No network sockets
//! - No ambient host environment variables
//! - No host filesystem access except one preopened directory: the active
//!   **plot** (default `~/Helix/plots/default`), mounted at guest path `/plot`
//!
//! Hands must never raw-dial the network (Switch is the sole egress). This
//! host does not enable any network WASI APIs.

use std::path::{Path, PathBuf};

use thiserror::Error;
use wasmtime::{Engine, Linker, Module, Store};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx};

/// Errors from loading or running a Hands module.
#[derive(Debug, Error)]
pub enum HandsError {
    #[error("wasm engine: {0}")]
    Engine(String),
    #[error("invalid wasm module: {0}")]
    Module(String),
    #[error("wasi setup: {0}")]
    Wasi(String),
    #[error("execution: {0}")]
    Exec(String),
    #[error("plot directory does not exist: {0}")]
    PlotMissing(PathBuf),
    #[error("plot path is not a directory: {0}")]
    PlotNotDir(PathBuf),
}

/// Configuration for a Hands run.
#[derive(Debug, Clone)]
pub struct HandsConfig {
    /// Host path to the active plot (preopened at guest `/plot`).
    pub plot_dir: PathBuf,
    /// Guest argv (first element is typically the module name).
    pub args: Vec<String>,
    /// Optional fuel limit (instruction budget). `None` = unlimited.
    pub fuel: Option<u64>,
    /// When true, inherit host stdout/stderr (CLI mode). Default false for
    /// library use so callers can capture via chronicle later.
    pub inherit_stdio: bool,
}

impl HandsConfig {
    /// Plot-scoped config with a default program name and no inherited stdio.
    pub fn for_plot(plot_dir: impl Into<PathBuf>) -> Self {
        Self {
            plot_dir: plot_dir.into(),
            args: vec!["hands".into()],
            fuel: None,
            inherit_stdio: false,
        }
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    pub fn with_fuel(mut self, fuel: u64) -> Self {
        self.fuel = Some(fuel);
        self
    }

    pub fn with_inherit_stdio(mut self, inherit: bool) -> Self {
        self.inherit_stdio = inherit;
        self
    }
}

/// Outcome of a module run (exit status only; stdio may be inherited).
#[derive(Debug, Clone)]
pub struct HandsResult {
    /// WASI exit status if the module called `proc_exit`; otherwise 0.
    pub exit_code: i32,
}

/// Run a core Wasm module (WASI preview1) with only the plot directory visible.
///
/// The module should export `_start` (wasi-sdk / `cargo wasi` command style).
/// Modules without `_start` are still linked and return exit 0 (probe path).
pub fn run_module(wasm: &[u8], config: &HandsConfig) -> Result<HandsResult, HandsError> {
    if !config.plot_dir.exists() {
        return Err(HandsError::PlotMissing(config.plot_dir.clone()));
    }
    if !config.plot_dir.is_dir() {
        return Err(HandsError::PlotNotDir(config.plot_dir.clone()));
    }

    let mut engine_config = wasmtime::Config::new();
    engine_config.cranelift_opt_level(wasmtime::OptLevel::Speed);
    if config.fuel.is_some() {
        engine_config.consume_fuel(true);
    }
    let engine = Engine::new(&engine_config).map_err(|e| HandsError::Engine(e.to_string()))?;

    let module = Module::new(&engine, wasm).map_err(|e| HandsError::Module(e.to_string()))?;

    let mut builder = WasiCtx::builder();
    // Deny-by-default: do not inherit env; do not preopen anything except plot.
    // Network is not enabled (no `inherit_network` / socket APIs wired).
    for a in &config.args {
        builder.arg(a);
    }
    if config.inherit_stdio {
        builder.inherit_stdout();
        builder.inherit_stderr();
    }
    builder
        .preopened_dir(
            &config.plot_dir,
            "/plot",
            DirPerms::READ | DirPerms::MUTATE,
            FilePerms::READ | FilePerms::WRITE,
        )
        .map_err(|e| HandsError::Wasi(e.to_string()))?;

    let wasi: WasiP1Ctx = builder.build_p1();

    let mut store = Store::new(&engine, wasi);
    if let Some(fuel) = config.fuel {
        store
            .set_fuel(fuel)
            .map_err(|e| HandsError::Engine(e.to_string()))?;
    }

    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    p1::add_to_linker_sync(&mut linker, |cx| cx).map_err(|e| HandsError::Wasi(e.to_string()))?;

    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| HandsError::Exec(e.to_string()))?;

    let exit_code = if let Ok(start) = instance.get_typed_func::<(), ()>(&mut store, "_start") {
        match start.call(&mut store, ()) {
            Ok(()) => 0,
            Err(e) => match extract_exit_status(&e) {
                Some(code) => code,
                None => return Err(HandsError::Exec(e.to_string())),
            },
        }
    } else {
        // No `_start` — module linked successfully (valid probe / library path).
        0
    };

    Ok(HandsResult { exit_code })
}

/// Resolve the default plot directory under a Helix home (`plots/default`).
pub fn default_plot_dir(home_root: &Path) -> PathBuf {
    home_root.join("plots").join("default")
}

fn extract_exit_status(err: &anyhow::Error) -> Option<i32> {
    for cause in err.chain() {
        let s = cause.to_string();
        // wasmtime_wasi::I32Exit and related display forms across versions.
        for prefix in [
            "Exited with i32 exit status ",
            "exit code: ",
            "I32Exit(",
        ] {
            if let Some(rest) = s.strip_prefix(prefix) {
                let digits: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '-')
                    .collect();
                if let Ok(code) = digits.parse::<i32>() {
                    return Some(code);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Minimal core module with no imports/exports.
    fn empty_module_bytes() -> Vec<u8> {
        // Manually encoded `(module)` — no wat crate dependency.
        // wasm magic + version + empty sections.
        vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
    }

    #[test]
    fn rejects_missing_plot() {
        let cfg = HandsConfig::for_plot("/nonexistent/helix-plot-xyz");
        let err = run_module(&empty_module_bytes(), &cfg).unwrap_err();
        assert!(matches!(err, HandsError::PlotMissing(_)));
    }

    #[test]
    fn loads_empty_module_against_plot() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.txt"), "hello plot").unwrap();
        let cfg = HandsConfig::for_plot(dir.path());
        let result = run_module(&empty_module_bytes(), &cfg).unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn default_plot_path() {
        let p = default_plot_dir(Path::new("/tmp/Helix"));
        assert_eq!(p, PathBuf::from("/tmp/Helix/plots/default"));
    }

    #[test]
    fn rejects_file_as_plot() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-dir");
        fs::write(&file, "x").unwrap();
        let cfg = HandsConfig::for_plot(&file);
        let err = run_module(&empty_module_bytes(), &cfg).unwrap_err();
        assert!(matches!(err, HandsError::PlotNotDir(_)));
    }
}
