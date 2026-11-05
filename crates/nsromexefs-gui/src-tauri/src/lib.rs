use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tauri::{Emitter, State, WebviewWindow};

#[derive(Default)]
struct AppState {
    child: Mutex<Option<Child>>,
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

#[tauri::command]
fn default_cli_path() -> Option<String> {
    let executable = std::env::current_exe().ok()?;
    for ancestor in executable.parent()?.ancestors() {
        for candidate in [
            ancestor.join(cli_filename()),
            ancestor.join("target").join("release").join(cli_filename()),
            ancestor.join("target").join("debug").join(cli_filename()),
        ] {
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

#[tauri::command]
fn validate_keys(cli_path: String, keys: Vec<String>) -> Result<Value, String> {
    if keys.is_empty() {
        return Err("Selecciona al menos un archivo de claves.".into());
    }
    let mut command = cli_command(&cli_path, &keys)?;
    command.args(["keys", "--check", "--json"]);
    run_json(command)
}

#[tauri::command]
fn inspect_inputs(
    cli_path: String,
    inputs: Vec<String>,
    keys: Vec<String>,
) -> Result<Value, String> {
    validate_inputs(&inputs)?;
    let mut command = cli_command(&cli_path, &keys)?;
    command.args(["info", "--json"]);
    command.args(inputs);
    run_json(command)
}

#[tauri::command]
fn plan_dump(
    cli_path: String,
    inputs: Vec<String>,
    keys: Vec<String>,
    options: DumpOptions,
) -> Result<Value, String> {
    validate_inputs(&inputs)?;
    if options.output_dir.trim().is_empty() {
        return Err("Selecciona una carpeta de destino.".into());
    }
    let mut command = cli_command(&cli_path, &keys)?;
    command.args(["dump", "--dry-run", "--json", "--out"]);
    command.arg(&options.output_dir);
    append_dump_options(&mut command, &options, false);
    command.args(inputs);
    run_json(command)
}

#[tauri::command]
fn run_dump(
    window: WebviewWindow,
    state: State<'_, AppState>,
    cli_path: String,
    inputs: Vec<String>,
    keys: Vec<String>,
    options: DumpOptions,
) -> Result<Value, String> {
    validate_inputs(&inputs)?;
    if options.output_dir.trim().is_empty() {
        return Err("Selecciona una carpeta de destino.".into());
    }

    let mut command = cli_command(&cli_path, &keys)?;
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
        let mut running = state
            .child
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
        let mut running = state
            .child
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
            let mut running = state
                .child
                .lock()
                .map_err(|_| "El estado de extracción no está disponible.".to_string())?;
            match running.as_mut() {
                Some(child) => child.try_wait(),
                None => return Err("La extracción se canceló.".into()),
            }
        };
        match result {
            Ok(Some(status)) => {
                if let Ok(mut running) = state.child.lock() {
                    running.take();
                }
                break status;
            }
            Ok(None) => thread::sleep(Duration::from_millis(120)),
            Err(error) => {
                if let Ok(mut running) = state.child.lock() {
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

fn cli_command(path: &str, keys: &[String]) -> Result<Command, String> {
    let cli = Path::new(path);
    if !cli.is_file() {
        return Err("No encuentro nsromexefs. Selecciona el ejecutable CLI en Ajustes.".into());
    }
    let mut command = Command::new(cli);
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
            default_cli_path,
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
