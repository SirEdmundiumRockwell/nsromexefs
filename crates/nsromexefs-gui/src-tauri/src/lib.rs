use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Emitter, State, WebviewWindow};

#[derive(Default, Clone)]
struct AppState {
    child: Arc<Mutex<Option<Child>>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DumpOptions {
    output_dir: String,
    tree: bool,
    resume: bool,
    force: bool,
    exefs_only: bool,
    romfs_only: bool,
    version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineInfo {
    path: String,
    version: Option<String>,
    bundled: bool,
    overridden: bool,
}

#[tauri::command]
async fn engine_info(cli_path: Option<String>) -> Result<EngineInfo, String> {
    tauri::async_runtime::spawn_blocking(move || engine_info_blocking(cli_path.as_deref()))
        .await
        .map_err(|error| format!("No se pudo consultar el motor: {error}"))?
}

fn engine_info_blocking(cli_path: Option<&str>) -> Result<EngineInfo, String> {
    let (path, overridden) = resolve_cli(cli_path)?;
    Ok(EngineInfo {
        version: read_cli_version(&path),
        bundled: !is_development_build(&path),
        overridden,
        path: path.to_string_lossy().into_owned(),
    })
}

/// Resolves the extraction engine. An explicit, existing override wins; otherwise the
/// binary bundled beside the GUI (or a local `target` build during development) is used.
fn resolve_cli(override_path: Option<&str>) -> Result<(PathBuf, bool), String> {
    if let Some(path) = override_path.and_then(valid_cli_override) {
        return Ok((path, true));
    }
    if let Some(path) = adjacent_cli_path() {
        return Ok((path, false));
    }
    if let Some(path) = development_cli_paths()
        .into_iter()
        .find(|path| path.is_file())
    {
        return Ok((path, false));
    }
    Err("No encuentro nsromexefs. Selecciona el ejecutable CLI en Ajustes.".into())
}

fn valid_cli_override(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    (path.is_file() && is_cli_file_name(&path)).then_some(path)
}

fn is_cli_file_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("nsromexefs") || name.eq_ignore_ascii_case("nsromexefs.exe")
        })
}

fn adjacent_cli_path() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let candidate = executable.parent()?.join(cli_filename());
    candidate.is_file().then_some(candidate)
}

fn development_cli_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let Ok(executable) = std::env::current_exe() else {
        return candidates;
    };
    let Some(parent) = executable.parent() else {
        return candidates;
    };
    for ancestor in parent.ancestors() {
        candidates.push(ancestor.join("target").join("release").join(cli_filename()));
        candidates.push(ancestor.join("target").join("debug").join(cli_filename()));
    }
    candidates
}

/// A binary under a Cargo `target` directory belongs to a development build, not to an
/// installed bundle. This only affects how the engine is described in the interface.
fn is_development_build(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str().eq_ignore_ascii_case("target"))
}

fn read_cli_version(path: &Path) -> Option<String> {
    let output = Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .last()
        .map(str::to_owned)
        .filter(|version| !version.is_empty())
}

#[tauri::command]
async fn validate_keys(cli_path: String, keys: Vec<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || validate_keys_blocking(cli_path, keys))
        .await
        .map_err(|error| format!("No se pudo validar el keyset: {error}"))?
}

fn validate_keys_blocking(cli_path: String, keys: Vec<String>) -> Result<Value, String> {
    if keys.is_empty() {
        return Err("Selecciona al menos un archivo de claves.".into());
    }
    let (cli, _) = resolve_cli(Some(cli_path.as_str()))?;
    let mut command = cli_command(&cli, &keys)?;
    command.args(["keys", "--check", "--json"]);
    run_json(command)
}

#[tauri::command]
async fn inspect_inputs(
    cli_path: String,
    inputs: Vec<String>,
    keys: Vec<String>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || inspect_inputs_blocking(cli_path, inputs, keys))
        .await
        .map_err(|error| format!("No se pudo analizar los contenedores: {error}"))?
}

fn inspect_inputs_blocking(
    cli_path: String,
    inputs: Vec<String>,
    keys: Vec<String>,
) -> Result<Value, String> {
    validate_inputs(&inputs)?;
    let (cli, _) = resolve_cli(Some(cli_path.as_str()))?;
    let mut command = cli_command(&cli, &keys)?;
    command.args(["info", "--json"]);
    command.args(inputs);
    run_json(command)
}

#[tauri::command]
async fn plan_dump(
    cli_path: String,
    inputs: Vec<String>,
    keys: Vec<String>,
    options: DumpOptions,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        plan_dump_blocking(cli_path, inputs, keys, options)
    })
    .await
    .map_err(|error| format!("No se pudo preparar el plan: {error}"))?
}

fn plan_dump_blocking(
    cli_path: String,
    inputs: Vec<String>,
    keys: Vec<String>,
    options: DumpOptions,
) -> Result<Value, String> {
    validate_inputs(&inputs)?;
    if options.output_dir.trim().is_empty() {
        return Err("Selecciona una carpeta de destino.".into());
    }
    let (cli, _) = resolve_cli(Some(cli_path.as_str()))?;
    let mut command = cli_command(&cli, &keys)?;
    command.args(["dump", "--dry-run", "--json", "--out"]);
    command.arg(&options.output_dir);
    append_dump_options(&mut command, &options, false);
    command.args(inputs);
    run_json(command)
}

#[tauri::command]
async fn run_dump(
    window: WebviewWindow,
    state: State<'_, AppState>,
    cli_path: String,
    inputs: Vec<String>,
    keys: Vec<String>,
    options: DumpOptions,
) -> Result<Value, String> {
    let child_state = state.child.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_dump_blocking(child_state, window, cli_path, inputs, keys, options)
    })
    .await
    .map_err(|error| format!("No se pudo completar la extracción: {error}"))?
}

fn run_dump_blocking(
    child_state: Arc<Mutex<Option<Child>>>,
    window: WebviewWindow,
    cli_path: String,
    inputs: Vec<String>,
    keys: Vec<String>,
    options: DumpOptions,
) -> Result<Value, String> {
    validate_inputs(&inputs)?;
    if options.output_dir.trim().is_empty() {
        return Err("Selecciona una carpeta de destino.".into());
    }

    let (cli, _) = resolve_cli(Some(cli_path.as_str()))?;
    let mut command = cli_command(&cli, &keys)?;
    command.args(["dump", "--json", "--out"]);
    command.arg(&options.output_dir);
    append_dump_options(&mut command, &options, true);
    command.args(inputs);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = command
        .spawn()
        .map_err(|error| format!("No se pudo iniciar nsromexefs: {error}"))?;
    let mut child = Some(child);
    {
        let mut running = child_state
            .lock()
            .map_err(|_| "El estado de extracción no está disponible.".to_string())?;
        if running.is_some() {
            if let Some(child) = child.as_mut() {
                let _ = child.kill();
            }
            return Err("Ya hay una extracción en curso.".into());
        }
        *running = child.take();
    }

    let (stdout, stderr) = {
        let mut running = child_state
            .lock()
            .map_err(|_| "El estado de extracción no está disponible.".to_string())?;
        let child = running
            .as_mut()
            .ok_or_else(|| "No hay un proceso de extracción activo.".to_string())?;
        (child.stdout.take(), child.stderr.take())
    };
    let stdout_thread = thread::spawn(move || {
        let mut output = String::new();
        if let Some(mut stdout) = stdout {
            let _ = stdout.read_to_string(&mut output);
        }
        output
    });
    let progress_window = window.clone();
    let stderr_thread = thread::spawn(move || {
        if let Some(stderr) = stderr {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = progress_window.emit("dump-log", line);
            }
        }
    });

    let _ = window.emit("dump-state", "running");
    let status = loop {
        let result = {
            let mut running = child_state
                .lock()
                .map_err(|_| "El estado de extracción no está disponible.".to_string())?;
            match running.as_mut() {
                Some(child) => child.try_wait(),
                None => return Err("La extracción se canceló.".into()),
            }
        };
        match result {
            Ok(Some(status)) => {
                if let Ok(mut running) = child_state.lock() {
                    running.take();
                }
                break status;
            }
            Ok(None) => thread::sleep(Duration::from_millis(120)),
            Err(error) => {
                if let Ok(mut running) = child_state.lock() {
                    if let Some(child) = running.as_mut() {
                        let _ = child.kill();
                    }
                    running.take();
                }
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(format!("No se pudo consultar el proceso: {error}"));
            }
        }
    };

    let stdout = stdout_thread
        .join()
        .map_err(|_| "Falló la lectura de la respuesta de nsromexefs.".to_string())?;
    let _ = stderr_thread.join();
    if !status.success() {
        let _ = window.emit("dump-state", "failed");
        let message = serde_json::from_str::<Value>(&stdout)
            .ok()
            .and_then(|value| value["error"]["message"].as_str().map(str::to_owned))
            .unwrap_or_else(|| {
                format!(
                    "La extracción terminó con código {}.",
                    status.code().unwrap_or(-1)
                )
            });
        return Err(message);
    }
    let result = serde_json::from_str(&stdout)
        .map_err(|error| format!("nsromexefs devolvió JSON no válido: {error}"))?;
    let _ = window.emit("dump-state", "done");
    Ok(result)
}

#[tauri::command]
fn cancel_dump(state: State<'_, AppState>) -> Result<(), String> {
    let mut running = state
        .child
        .lock()
        .map_err(|_| "El estado de extracción no está disponible.".to_string())?;
    let child = running
        .as_mut()
        .ok_or_else(|| "No hay ninguna extracción en curso.".to_string())?;
    child
        .kill()
        .map_err(|error| format!("No se pudo cancelar la extracción: {error}"))
}

fn cli_command(path: &Path, keys: &[String]) -> Result<Command, String> {
    if !path.is_file() {
        return Err("No encuentro nsromexefs. Selecciona el ejecutable CLI en Ajustes.".into());
    }
    let mut command = Command::new(path);
    for key in keys {
        command.arg("-k").arg(key);
    }
    Ok(command)
}

fn run_json(mut command: Command) -> Result<Value, String> {
    let output = command
        .output()
        .map_err(|error| format!("No se pudo ejecutar nsromexefs: {error}"))?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = serde_json::from_str::<Value>(&stdout)
            .ok()
            .and_then(|value| value["error"]["message"].as_str().map(str::to_owned))
            .unwrap_or_else(|| String::from_utf8_lossy(&output.stderr).trim().to_owned());
        return Err(if message.is_empty() {
            format!(
                "nsromexefs terminó con código {}.",
                output.status.code().unwrap_or(-1)
            )
        } else {
            message
        });
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("nsromexefs devolvió JSON no válido: {error}"))
}

fn append_dump_options(command: &mut Command, options: &DumpOptions, include_resume: bool) {
    if options.tree {
        command.arg("--tree");
    }
    if options.exefs_only {
        command.arg("--exefs-only");
    }
    if options.romfs_only {
        command.arg("--romfs-only");
    }
    if let Some(version) = options.version.as_deref().filter(|value| !value.is_empty()) {
        command.arg("--version").arg(version);
    }
    if include_resume && options.resume {
        command.arg("--resume");
    }
    if options.force {
        command.arg("--force");
    }
}

fn validate_inputs(inputs: &[String]) -> Result<(), String> {
    if inputs.is_empty() || inputs.len() > 2 {
        return Err("Selecciona uno o dos contenedores NSP/XCI.".into());
    }
    for input in inputs {
        if !PathBuf::from(input).is_file() {
            return Err(format!("No encuentro el archivo: {input}"));
        }
    }
    Ok(())
}

fn cli_filename() -> &'static str {
    if cfg!(windows) {
        "nsromexefs.exe"
    } else {
        "nsromexefs"
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            engine_info,
            validate_keys,
            inspect_inputs,
            plan_dump,
            run_dump,
            cancel_dump,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the nsromexefs GUI");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_targets_use_platform_binary_name() {
        assert!(matches!(cli_filename(), "nsromexefs" | "nsromexefs.exe"));
    }

    #[test]
    fn recognizes_only_the_engine_file_names() {
        assert!(is_cli_file_name(Path::new("nsromexefs")));
        assert!(is_cli_file_name(Path::new("NSROMEXEFS.EXE")));
        assert!(!is_cli_file_name(Path::new("other.exe")));
        assert!(!is_cli_file_name(Path::new("nsromexefs.bak")));
    }

    #[test]
    fn ignores_empty_or_missing_overrides() {
        assert!(valid_cli_override("").is_none());
        assert!(valid_cli_override("   ").is_none());
        assert!(valid_cli_override("this-file-does-not-exist-nsromexefs").is_none());
    }

    #[test]
    fn detects_cargo_target_directories_as_development_builds() {
        assert!(is_development_build(Path::new(
            "repo/target/debug/nsromexefs.exe"
        )));
        assert!(!is_development_build(Path::new(
            "Program Files/nsromexefs/nsromexefs.exe"
        )));
    }

    #[test]
    fn rejects_empty_or_oversized_input_lists() {
        assert!(validate_inputs(&[]).is_err());
        assert!(validate_inputs(&["a".into(), "b".into(), "c".into()]).is_err());
    }

    #[test]
    fn dry_run_options_pass_force_but_not_resume() {
        let options = DumpOptions {
            output_dir: "out".into(),
            tree: true,
            resume: true,
            force: true,
            exefs_only: false,
            romfs_only: false,
            version: Some("1.0.0".into()),
        };
        let mut command = Command::new("nsromexefs");
        append_dump_options(&mut command, &options, false);
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--tree".into()));
        assert!(!args.contains(&"--resume".into()));
        assert!(args.contains(&"--force".into()));
    }
}
