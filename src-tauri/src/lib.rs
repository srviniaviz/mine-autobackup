use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Duration, Local};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{copy, Read, Seek, SeekFrom, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{mpsc, Mutex},
    thread,
    time::Duration as StdDuration,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WebviewWindow,
};
use tauri_plugin_positioner::{Position, WindowExt};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const KEYRING_SERVICE: &str = "Mine AutoBackup";
const GOOGLE_REFRESH_TOKEN_ACCOUNT: &str = "google-refresh-token";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    minecraft_dir: Option<PathBuf>,
    backup_dir: Option<PathBuf>,
    #[serde(default)]
    selected_worlds: Vec<String>,
    #[serde(default)]
    google: GoogleConfig,
    interval_minutes: u64,
    auto_enabled: bool,
    last_backup_at: Option<DateTime<Local>>,
    next_backup_at: Option<DateTime<Local>>,
    last_result: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            minecraft_dir: default_minecraft_dir(),
            backup_dir: default_drive_backup_dir(),
            selected_worlds: Vec::new(),
            google: GoogleConfig::default(),
            interval_minutes: 60,
            auto_enabled: false,
            last_backup_at: None,
            next_backup_at: None,
            last_result: None,
        }
    }
}

#[derive(Debug, Serialize)]
struct BackupStatus {
    minecraft_dir: Option<String>,
    backup_dir: Option<String>,
    worlds: Vec<MinecraftWorld>,
    selected_worlds: Vec<String>,
    google_connected: bool,
    google_email: Option<String>,
    interval_minutes: u64,
    auto_enabled: bool,
    is_running: bool,
    progress: BackupProgress,
    running_since: Option<String>,
    last_backup_at: Option<String>,
    next_backup_at: Option<String>,
    last_result: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct BackupProgress {
    current: u64,
    total: u64,
    label: String,
    updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct MinecraftWorld {
    id: String,
    name: String,
    size_bytes: u64,
    modified_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GoogleConfig {
    client_id: Option<String>,
    refresh_token: Option<String>,
    access_token: Option<String>,
    expires_at: Option<DateTime<Local>>,
    email: Option<String>,
    drive_folder_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<i64>,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DriveFileResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct DriveFilesResponse {
    files: Vec<DriveFileResponse>,
}

struct RuntimeState {
    config: AppConfig,
    is_running: bool,
    progress: BackupProgress,
    running_since: Option<DateTime<Local>>,
}

struct AppState {
    inner: Mutex<RuntimeState>,
    config_path: PathBuf,
}

#[tauri::command]
fn get_status(state: tauri::State<AppState>) -> Result<BackupStatus, String> {
    let inner = state
        .inner
        .lock()
        .map_err(|_| "Estado travado".to_string())?;
    Ok(status_from_state(&inner))
}

#[tauri::command]
fn set_minecraft_dir(path: String, state: tauri::State<AppState>) -> Result<(), String> {
    let candidate = PathBuf::from(path);
    let saves = candidate.join("saves");
    if !saves.is_dir() {
        return Err("Essa pasta nao parece conter .minecraft/saves".into());
    }

    update_config(&state, |config| {
        config.minecraft_dir = Some(candidate);
        sync_selected_worlds(config);
        refresh_next_backup(config);
    })
}

#[tauri::command]
fn set_backup_dir(path: String, state: tauri::State<AppState>) -> Result<(), String> {
    let candidate = PathBuf::from(path);
    fs::create_dir_all(&candidate).map_err(|error| error.to_string())?;

    update_config(&state, |config| {
        config.backup_dir = Some(candidate);
        refresh_next_backup(config);
    })
}

#[tauri::command]
fn set_interval_minutes(minutes: u64, state: tauri::State<AppState>) -> Result<(), String> {
    let minutes = minutes.clamp(5, 24 * 60);
    update_config(&state, |config| {
        config.interval_minutes = minutes;
        refresh_next_backup(config);
    })
}

#[tauri::command]
fn set_auto_enabled(enabled: bool, state: tauri::State<AppState>) -> Result<(), String> {
    update_config(&state, |config| {
        config.auto_enabled = enabled;
        refresh_next_backup(config);
    })
}

#[tauri::command]
fn set_selected_worlds(worlds: Vec<String>, state: tauri::State<AppState>) -> Result<(), String> {
    update_config(&state, |config| {
        let available = available_world_ids(config);
        config.selected_worlds = worlds
            .into_iter()
            .filter(|world| available.contains(world))
            .collect();
        refresh_next_backup(config);
    })
}

#[tauri::command]
fn google_login(app: tauri::AppHandle) -> Result<(), String> {
    // OAuth finishes in the background; the frontend polls get_status.
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut config = match state.inner.lock() {
            Ok(inner) => inner.config.google.clone(),
            Err(_) => return,
        };

        let result = perform_google_login(&mut config);
        if let Ok(mut inner) = state.inner.lock() {
            match result {
                Ok(()) => {
                    inner.config.google = config;
                    match save_config(&state.config_path, &inner.config) {
                        Ok(()) => {
                            inner.config.last_result = Some("Google Drive conectado".into());
                        }
                        Err(error) => {
                            inner.config.google = GoogleConfig::default();
                            inner.config.last_result =
                                Some(format!("Falha ao salvar login Google: {error}"));
                            let _ = save_config(&state.config_path, &inner.config);
                        }
                    }
                }
                Err(error) => {
                    inner.config.last_result = Some(format!("Falha no login Google: {error}"));
                    let _ = save_config(&state.config_path, &inner.config);
                }
            }
        }

        if let Some(window) = app.get_webview_window("main") {
            show_panel(&window);
        }
    });
    Ok(())
}

#[tauri::command]
fn google_logout(state: tauri::State<AppState>) -> Result<(), String> {
    update_config(&state, |config| {
        config.google.refresh_token = None;
        config.google.access_token = None;
        config.google.expires_at = None;
        config.google.email = None;
        config.google.drive_folder_id = None;
    })
}

#[tauri::command]
fn hide_window(window: WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|error| error.to_string())
}

#[tauri::command]
async fn run_backup_now(app: tauri::AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        run_backup(&state)
    })
    .await
    .map_err(|error| format!("Backup interrompido: {error}"))?
}

pub fn run() {
    let config_path = config_path();
    let mut config = load_config(&config_path).unwrap_or_default();
    let had_disk_google_tokens =
        config.google.refresh_token.is_some() || config.google.access_token.is_some();
    load_secure_google_tokens(&mut config);
    if had_disk_google_tokens {
        let _ = save_config(&config_path, &config);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_positioner::init())
        .manage(AppState {
            inner: Mutex::new(RuntimeState {
                config,
                is_running: false,
                progress: BackupProgress::default(),
                running_since: None,
            }),
            config_path,
        })
        .setup(|app| {
            #[cfg(desktop)]
            {
                use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

                app.handle()
                    .plugin(tauri_plugin_autostart::init(
                        MacosLauncher::LaunchAgent,
                        None,
                    ))
                    .map_err(|error| error.to_string())?;

                let _ = app.autolaunch().enable();
            }

            setup_tray(app)?;
            start_scheduler(app.handle().clone());
            if let Some(window) = app.get_webview_window("main") {
                show_panel(&window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            set_minecraft_dir,
            set_backup_dir,
            set_interval_minutes,
            set_auto_enabled,
            set_selected_worlds,
            google_login,
            google_logout,
            run_backup_now,
            hide_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let open_i = MenuItem::with_id(app, "open", "Abrir", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_i, &quit_i])?;

    TrayIconBuilder::new()
        .tooltip("Mine AutoBackup")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    show_panel(&window);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    show_panel(&window);
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn show_panel(window: &WebviewWindow) {
    let _ = window.as_ref().window().move_window(Position::BottomRight);
    let _ = window.show();
    let _ = window.set_focus();
}

fn start_scheduler(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            ticker.tick().await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };

            let should_run = {
                let Ok(inner) = state.inner.lock() else {
                    continue;
                };
                inner.config.auto_enabled
                    && !inner.is_running
                    && inner
                        .config
                        .next_backup_at
                        .map(|next| next <= Local::now())
                        .unwrap_or(false)
            };

            if should_run {
                let backup_app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Some(state) = backup_app.try_state::<AppState>() {
                        let _ = run_backup(&state);
                    }
                });
            }
        }
    });
}

fn run_backup(state: &AppState) -> Result<String, String> {
    let (minecraft_dir, backup_dir, selected_worlds) = {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| "Estado travado".to_string())?;
        if inner.is_running {
            return Err("Ja existe um backup em andamento".into());
        }
        sync_selected_worlds(&mut inner.config);
        let minecraft_dir = inner
            .config
            .minecraft_dir
            .clone()
            .ok_or_else(|| "Escolha a pasta .minecraft primeiro".to_string())?;
        let backup_dir = inner
            .config
            .backup_dir
            .clone()
            .unwrap_or_else(default_staging_backup_dir);
        if inner.config.google.refresh_token.is_none() {
            return Err("Conecte sua conta Google Drive primeiro".into());
        }
        if inner.config.selected_worlds.is_empty() {
            return Err("Escolha pelo menos um mundo para backup".into());
        }
        let selected_worlds = inner.config.selected_worlds.clone();
        inner.is_running = true;
        inner.running_since = Some(Local::now());
        inner.progress = BackupProgress::default();
        inner.config.last_result = None;
        (minecraft_dir, backup_dir, selected_worlds)
    };

    set_progress(
        state,
        BackupProgress {
            current: 0,
            total: 1,
            label: "Preparando backup".into(),
            updated_at: None,
        },
    );

    let result = create_backup_archive(state, &minecraft_dir, &backup_dir, &selected_worlds)
        .and_then(|path| upload_archive_if_connected(state, path));
    let mut inner = state
        .inner
        .lock()
        .map_err(|_| "Estado travado".to_string())?;
    inner.is_running = false;
    inner.progress = BackupProgress::default();
    inner.running_since = None;

    match result {
        Ok(path) => {
            let message = format!("Backup criado: {}", path.display());
            inner.config.last_backup_at = Some(Local::now());
            inner.config.last_result = Some(message.clone());
            refresh_next_backup(&mut inner.config);
            save_config(&state.config_path, &inner.config)?;
            Ok(message)
        }
        Err(error) => {
            inner.config.last_result = Some(error.clone());
            save_config(&state.config_path, &inner.config)?;
            Err(error)
        }
    }
}

fn create_backup_archive(
    state: &AppState,
    minecraft_dir: &Path,
    backup_dir: &Path,
    selected_worlds: &[String],
) -> Result<PathBuf, String> {
    let saves_dir = minecraft_dir.join("saves");
    if !saves_dir.is_dir() {
        return Err("Nao encontrei a pasta saves dentro da .minecraft".into());
    }

    let available = list_worlds_from_dir(&saves_dir)?;
    let available_ids: HashSet<_> = available.into_iter().map(|world| world.id).collect();
    let selected: Vec<_> = selected_worlds
        .iter()
        .filter(|world| available_ids.contains(*world))
        .collect();
    if selected.is_empty() {
        return Err("Nenhum mundo selecionado foi encontrado na pasta saves".into());
    }

    let total_files = selected.iter().try_fold(0_u64, |count, world| {
        count_world_files(&saves_dir.join(world)).map(|world_count| count + world_count)
    })?;
    set_progress(
        state,
        BackupProgress {
            current: 0,
            total: total_files.max(1),
            label: "Compactando mundos".into(),
            updated_at: None,
        },
    );

    fs::create_dir_all(backup_dir).map_err(|error| error.to_string())?;
    let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
    let archive_path = backup_dir.join(format!("minecraft-worlds-{timestamp}.zip"));
    let archive_file = File::create(&archive_path).map_err(|error| error.to_string())?;
    let mut zip = ZipWriter::new(archive_file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut written_files = 0_u64;

    for world in selected {
        let world_path = saves_dir.join(world);
        zip_directory(
            state,
            &saves_dir,
            &world_path,
            &mut zip,
            options,
            total_files.max(1),
            &mut written_files,
        )?;
    }
    zip.finish().map_err(|error| error.to_string())?;
    Ok(archive_path)
}

fn zip_directory<W: Write + Seek>(
    state: &AppState,
    root: &Path,
    current: &Path,
    zip: &mut ZipWriter<W>,
    options: SimpleFileOptions,
    total_files: u64,
    written_files: &mut u64,
) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if is_symlink_or_reparse_point(&metadata) {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/");

        if metadata.is_dir() {
            if !relative.is_empty() {
                zip.add_directory(format!("{relative}/"), options)
                    .map_err(|error| error.to_string())?;
            }
            zip_directory(state, root, &path, zip, options, total_files, written_files)?;
        } else if metadata.is_file() {
            zip.start_file(&relative, options)
                .map_err(|error| error.to_string())?;
            let mut file = File::open(&path).map_err(|error| error.to_string())?;
            copy(&mut file, zip).map_err(|error| error.to_string())?;
            *written_files += 1;
            set_progress(
                state,
                BackupProgress {
                    current: *written_files,
                    total: total_files,
                    label: progress_label(&relative),
                    updated_at: None,
                },
            );
        }
    }

    Ok(())
}

fn count_world_files(path: &Path) -> Result<u64, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if is_symlink_or_reparse_point(&metadata) {
        return Ok(0);
    }

    if metadata.is_file() {
        return Ok(1);
    }

    let mut count = 0;
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        count += count_world_files(&path)?;
    }
    Ok(count)
}

fn directory_size(path: &Path) -> Result<u64, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if is_symlink_or_reparse_point(&metadata) {
        return Ok(0);
    }

    if metadata.is_file() {
        return Ok(metadata.len());
    }

    let mut size = 0;
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        size += directory_size(&path)?;
    }
    Ok(size)
}

fn is_symlink_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        return metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }

    #[cfg(not(windows))]
    {
        false
    }
}

fn progress_label(relative: &str) -> String {
    let normalized = relative.replace('\\', "/");
    let mut parts = normalized.split('/');
    let world = parts.next().unwrap_or("mundo");
    let file = normalized
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(world);

    if world == file {
        format!("Compactando {world}")
    } else {
        format!("Compactando {world} / {file}")
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 || value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn status_from_state(inner: &RuntimeState) -> BackupStatus {
    let worlds = inner
        .config
        .minecraft_dir
        .as_ref()
        .map(|path| list_worlds_from_dir(&path.join("saves")).unwrap_or_default())
        .unwrap_or_default();

    BackupStatus {
        minecraft_dir: inner
            .config
            .minecraft_dir
            .as_ref()
            .map(|path| path.display().to_string()),
        backup_dir: inner
            .config
            .backup_dir
            .as_ref()
            .map(|path| path.display().to_string()),
        worlds,
        selected_worlds: inner.config.selected_worlds.clone(),
        google_connected: inner.config.google.refresh_token.is_some(),
        google_email: inner.config.google.email.clone(),
        interval_minutes: inner.config.interval_minutes,
        auto_enabled: inner.config.auto_enabled,
        is_running: inner.is_running,
        progress: inner.progress.clone(),
        running_since: inner.running_since.map(|value| value.to_rfc3339()),
        last_backup_at: inner.config.last_backup_at.map(|value| value.to_rfc3339()),
        next_backup_at: inner.config.next_backup_at.map(|value| value.to_rfc3339()),
        last_result: inner.config.last_result.clone(),
    }
}

fn set_progress(state: &AppState, progress: BackupProgress) {
    if let Ok(mut inner) = state.inner.lock() {
        inner.progress = BackupProgress {
            updated_at: Some(Local::now().to_rfc3339()),
            ..progress
        };
    }
}

fn update_config<F>(state: &AppState, update: F) -> Result<(), String>
where
    F: FnOnce(&mut AppConfig),
{
    let mut inner = state
        .inner
        .lock()
        .map_err(|_| "Estado travado".to_string())?;
    update(&mut inner.config);
    save_config(&state.config_path, &inner.config)
}

fn refresh_next_backup(config: &mut AppConfig) {
    config.next_backup_at = if config.auto_enabled {
        Some(Local::now() + Duration::minutes(config.interval_minutes as i64))
    } else {
        None
    };
}

fn sync_selected_worlds(config: &mut AppConfig) {
    let available = available_world_ids(config);
    config
        .selected_worlds
        .retain(|world| available.contains(world));
}

fn available_world_ids(config: &AppConfig) -> HashSet<String> {
    config
        .minecraft_dir
        .as_ref()
        .map(|path| list_worlds_from_dir(&path.join("saves")).unwrap_or_default())
        .unwrap_or_default()
        .into_iter()
        .map(|world| world.id)
        .collect()
}

fn list_worlds_from_dir(saves_dir: &Path) -> Result<Vec<MinecraftWorld>, String> {
    if !saves_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut worlds = Vec::new();
    for entry in fs::read_dir(saves_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if is_symlink_or_reparse_point(&metadata)
            || !metadata.is_dir()
            || !path.join("level.dat").is_file()
        {
            continue;
        }

        let id = entry.file_name().to_string_lossy().to_string();
        let modified_at = metadata
            .modified()
            .ok()
            .map(DateTime::<Local>::from)
            .map(|value| value.to_rfc3339());

        worlds.push(MinecraftWorld {
            name: id.clone(),
            id,
            size_bytes: directory_size(&path).unwrap_or(0),
            modified_at,
        });
    }

    worlds.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(worlds)
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("Mine AutoBackup")
        .join("config.json")
}

fn load_config(path: &Path) -> Option<AppConfig> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    let persist_result = persist_secure_google_tokens(config);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut disk_config = config.clone();
    disk_config.google.refresh_token = None;
    disk_config.google.access_token = None;
    disk_config.google.expires_at = None;
    let content = serde_json::to_string_pretty(&disk_config).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())?;
    persist_result
}

fn load_secure_google_tokens(config: &mut AppConfig) {
    config.google.access_token = None;
    config.google.expires_at = None;

    if config.google.refresh_token.is_some() {
        return;
    }

    let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, GOOGLE_REFRESH_TOKEN_ACCOUNT) else {
        return;
    };
    let Ok(token) = entry.get_password() else {
        return;
    };
    if !token.trim().is_empty() {
        config.google.refresh_token = Some(token);
    }
}

fn persist_secure_google_tokens(config: &AppConfig) -> Result<(), String> {
    if let Some(token) = config.google.refresh_token.as_deref() {
        let entry = keyring::Entry::new(KEYRING_SERVICE, GOOGLE_REFRESH_TOKEN_ACCOUNT)
            .map_err(|error| format!("Falha ao abrir cofre do Windows: {error}"))?;
        entry
            .set_password(token)
            .map_err(|error| format!("Falha ao salvar token Google no cofre do Windows: {error}"))?;
    } else {
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, GOOGLE_REFRESH_TOKEN_ACCOUNT) {
            let _ = entry.delete_credential();
        }
    }

    Ok(())
}

fn perform_google_login(google: &mut GoogleConfig) -> Result<(), String> {
    let client_id = configured_google_client_id(google)?;
    let client_secret = configured_google_client_secret();
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let verifier = random_string(96);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_string(32);
    let scope = "https://www.googleapis.com/auth/drive.file";
    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&code_challenge={}&code_challenge_method=S256&state={}",
        encode_component(&client_id),
        encode_component(&redirect_uri),
        encode_component(scope),
        encode_component(&challenge),
        encode_component(&state)
    );

    open_system_browser(&auth_url)?;
    let (code, returned_state) = wait_for_oauth_callback(listener)?;
    if returned_state.as_deref() != Some(state.as_str()) {
        return Err("Resposta OAuth invalida. Tente conectar novamente.".into());
    }

    let client = http_client()?;
    let mut form = vec![
        ("client_id", client_id.as_str()),
        ("code", code.as_str()),
        ("code_verifier", verifier.as_str()),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri.as_str()),
    ];
    if let Some(secret) = client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let token: TokenResponse = token_request(
        client
            .post("https://oauth2.googleapis.com/token")
            .form(&form),
    )?;

    google.access_token = Some(token.access_token.clone());
    google.refresh_token = token.refresh_token.or_else(|| google.refresh_token.clone());
    google.expires_at = token
        .expires_in
        .map(|seconds| Local::now() + Duration::seconds(seconds.saturating_sub(60)));
    google.email = Some("Conta conectada".into());
    Ok(())
}

fn upload_archive_if_connected(state: &AppState, archive_path: PathBuf) -> Result<PathBuf, String> {
    let mut google = {
        let inner = state
            .inner
            .lock()
            .map_err(|_| "Estado travado".to_string())?;
        inner.config.google.clone()
    };

    if google.refresh_token.is_none() {
        return Ok(archive_path);
    }

    set_progress(
        state,
        BackupProgress {
            current: 0,
            total: 100,
            label: "Preparando Google Drive".into(),
            updated_at: None,
        },
    );
    upload_archive_to_drive(state, &mut google, &archive_path)?;
    set_progress(
        state,
        BackupProgress {
            current: 100,
            total: 100,
            label: "Upload concluido".into(),
            updated_at: None,
        },
    );
    update_config(state, |config| {
        config.google = google;
    })?;
    Ok(archive_path)
}

fn upload_archive_to_drive(
    state: &AppState,
    google: &mut GoogleConfig,
    archive_path: &Path,
) -> Result<(), String> {
    set_progress(
        state,
        BackupProgress {
            current: 5,
            total: 100,
            label: "Autenticando Drive".into(),
            updated_at: None,
        },
    );
    let access_token = ensure_access_token_with_timeout(google, StdDuration::from_secs(15))?;
    let control_client = http_client()?;
    set_progress(
        state,
        BackupProgress {
            current: 10,
            total: 100,
            label: "Localizando pasta no Drive".into(),
            updated_at: None,
        },
    );
    let folder_id = ensure_drive_folder_with_timeout(
        control_client.clone(),
        access_token.clone(),
        google,
        StdDuration::from_secs(15),
    )?;
    let name = archive_path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| "Arquivo de backup invalido".to_string())?;
    let file_size = archive_path
        .metadata()
        .map_err(|error| error.to_string())?
        .len();
    let metadata = serde_json::json!({
        "name": name,
        "parents": [folder_id]
    });

    set_progress(
        state,
        BackupProgress {
            current: 0,
            total: file_size.max(1),
            label: "Iniciando envio".into(),
            updated_at: None,
        },
    );

    let session_url = control_client
        .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable")
        .bearer_auth(access_token)
        .header("Content-Type", "application/json; charset=UTF-8")
        .header("X-Upload-Content-Type", "application/zip")
        .header("X-Upload-Content-Length", file_size)
        .header("Content-Length", metadata.to_string().len())
        .body(metadata.to_string())
        .send()
        .map_err(|error| format!("Falha ao iniciar upload no Google Drive: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Google Drive recusou o inicio do upload: {error}"))?
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| "Google Drive nao retornou URL de upload".to_string())?;

    let upload_client = upload_http_client()?;
    upload_file_chunks(state, &upload_client, &session_url, archive_path, file_size)?;
    Ok(())
}

fn upload_file_chunks(
    state: &AppState,
    client: &reqwest::blocking::Client,
    session_url: &str,
    archive_path: &Path,
    file_size: u64,
) -> Result<(), String> {
    const CHUNK_SIZE: usize = 8 * 1024 * 1024;

    if file_size == 0 {
        return Err("Arquivo de backup vazio".into());
    }

    let mut file = File::open(archive_path).map_err(|error| error.to_string())?;
    let mut uploaded = 0_u64;
    let mut buffer = vec![0_u8; CHUNK_SIZE];

    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }

        let start = uploaded;
        let end = uploaded + read as u64 - 1;
        let chunk = buffer[..read].to_vec();

        set_progress(
            state,
            BackupProgress {
                current: uploaded,
                total: file_size,
                label: format!(
                    "Enviando {} de {}",
                    format_bytes(uploaded),
                    format_bytes(file_size)
                ),
                updated_at: None,
            },
        );

        let response = client
            .put(session_url)
            .header("Content-Length", read)
            .header("Content-Range", format!("bytes {start}-{end}/{file_size}"))
            .body(chunk)
            .send()
            .map_err(|error| format!("Falha ao enviar bloco para o Google Drive: {error}"))?;

        if response.status().is_success() {
            uploaded = end + 1;
        } else if response.status().as_u16() == 308 {
            uploaded = uploaded_from_range(response.headers())
                .map(|confirmed| confirmed + 1)
                .unwrap_or(end + 1);
            if uploaded < end + 1 {
                file.seek(SeekFrom::Start(uploaded))
                    .map_err(|error| error.to_string())?;
            }
        } else {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(format!(
                "Google Drive recusou bloco do upload: {status} {body}"
            ));
        }

        set_progress(
            state,
            BackupProgress {
                current: uploaded,
                total: file_size,
                label: format!(
                    "Enviando {} de {}",
                    format_bytes(uploaded),
                    format_bytes(file_size)
                ),
                updated_at: None,
            },
        );
    }

    Ok(())
}

fn uploaded_from_range(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let range = headers.get("range")?.to_str().ok()?;
    let (_, end) = range.strip_prefix("bytes=")?.split_once('-')?;
    end.parse().ok()
}

fn ensure_access_token(google: &mut GoogleConfig) -> Result<String, String> {
    if let (Some(token), Some(expires_at)) = (&google.access_token, google.expires_at) {
        if expires_at > Local::now() {
            return Ok(token.clone());
        }
    }

    let client_id = google
        .client_id
        .clone()
        .or_else(|| option_env!("GOOGLE_OAUTH_CLIENT_ID").map(str::to_string))
        .or_else(|| std::env::var("GOOGLE_OAUTH_CLIENT_ID").ok())
        .ok_or_else(|| "O app ainda nao tem Google OAuth Client ID embutido.".to_string())?;
    let client_secret = configured_google_client_secret();
    let refresh_token = google
        .refresh_token
        .clone()
        .ok_or_else(|| "Conecte sua conta Google primeiro".to_string())?;
    let client = http_client()?;
    let mut form = vec![
        ("client_id", client_id.as_str()),
        ("refresh_token", refresh_token.as_str()),
        ("grant_type", "refresh_token"),
    ];
    if let Some(secret) = client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let token: TokenResponse = token_request(
        client
            .post("https://oauth2.googleapis.com/token")
            .form(&form),
    )?;

    google.access_token = Some(token.access_token.clone());
    google.expires_at = token
        .expires_in
        .map(|seconds| Local::now() + Duration::seconds(seconds.saturating_sub(60)));
    Ok(token.access_token)
}

fn ensure_access_token_with_timeout(
    google: &mut GoogleConfig,
    timeout: StdDuration,
) -> Result<String, String> {
    let mut worker_google = google.clone();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = ensure_access_token(&mut worker_google).map(|token| (token, worker_google));
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok((token, updated_google))) => {
            *google = updated_google;
            Ok(token)
        }
        Ok(Err(error)) => Err(error),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err("Tempo limite ao autenticar no Google Drive. Tente reconectar a conta.".into())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("Autenticacao do Google Drive foi interrompida".into())
        }
    }
}

fn ensure_drive_folder(
    client: &reqwest::blocking::Client,
    access_token: &str,
    google: &mut GoogleConfig,
) -> Result<String, String> {
    if let Some(folder_id) = &google.drive_folder_id {
        return Ok(folder_id.clone());
    }

    let query = "name='Mine AutoBackup' and mimeType='application/vnd.google-apps.folder' and trashed=false";
    let response: DriveFilesResponse = client
        .get("https://www.googleapis.com/drive/v3/files")
        .bearer_auth(access_token)
        .query(&[("q", query), ("fields", "files(id,name)")])
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;

    if let Some(file) = response.files.first() {
        google.drive_folder_id = Some(file.id.clone());
        return Ok(file.id.clone());
    }

    let created: DriveFileResponse = client
        .post("https://www.googleapis.com/drive/v3/files")
        .bearer_auth(access_token)
        .json(&serde_json::json!({
            "name": "Mine AutoBackup",
            "mimeType": "application/vnd.google-apps.folder"
        }))
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    google.drive_folder_id = Some(created.id.clone());
    Ok(created.id)
}

fn ensure_drive_folder_with_timeout(
    client: reqwest::blocking::Client,
    access_token: String,
    google: &mut GoogleConfig,
    timeout: StdDuration,
) -> Result<String, String> {
    let mut worker_google = google.clone();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = ensure_drive_folder(&client, &access_token, &mut worker_google)
            .map(|folder_id| (folder_id, worker_google));
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok((folder_id, updated_google))) => {
            *google = updated_google;
            Ok(folder_id)
        }
        Ok(Err(error)) => Err(error),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err("Tempo limite ao preparar a pasta no Google Drive. Tente novamente.".into())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("Preparacao da pasta no Google Drive foi interrompida".into())
        }
    }
}

fn wait_for_oauth_callback(listener: TcpListener) -> Result<(String, Option<String>), String> {
    let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
    let mut buffer = [0; 4096];
    let size = stream
        .read(&mut buffer)
        .map_err(|error| error.to_string())?;
    let request = String::from_utf8_lossy(&buffer[..size]);
    let first_line = request.lines().next().unwrap_or_default();
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "Callback OAuth invalido".to_string())?;
    let query = path.split_once('?').map(|(_, query)| query).unwrap_or("");
    let code =
        query_param(query, "code").ok_or_else(|| "Login cancelado ou sem codigo".to_string())?;
    let state = query_param(query, "state");

    let html = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<html><body><h2>Mine AutoBackup conectado.</h2><p>Voce ja pode voltar para o app.</p></body></html>";
    let _ = stream.write_all(html.as_bytes());
    Ok((code, state))
}

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|error| error.to_string())
}

fn upload_http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15 * 60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())
}

fn token_request(builder: reqwest::blocking::RequestBuilder) -> Result<TokenResponse, String> {
    let response = builder.send().map_err(|error| error.to_string())?;
    let status = response.status();
    let body = response.text().map_err(|error| error.to_string())?;
    if !status.is_success() {
        if let Ok(error) = serde_json::from_str::<TokenErrorResponse>(&body) {
            let code = error.error.unwrap_or_else(|| "erro_desconhecido".into());
            if let Some(description) = error.error_description {
                return Err(format!("Google token HTTP {status}: {code} - {description}"));
            }
            return Err(format!("Google token HTTP {status}: {code}"));
        }
        return Err(format!("Google token HTTP {status}: {body}"));
    }
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

fn open_system_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        if name == key {
            Some(decode_component(value))
        } else {
            None
        }
    })
}

fn encode_component(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn decode_component(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.as_bytes().iter().copied().peekable();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let high = chars.next().unwrap_or(b'0') as char;
            let low = chars.next().unwrap_or(b'0') as char;
            if let Ok(decoded) = u8::from_str_radix(&format!("{high}{low}"), 16) {
                output.push(decoded as char);
            }
        } else if byte == b'+' {
            output.push(' ');
        } else {
            output.push(byte as char);
        }
    }
    output
}

fn random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn configured_google_client_id(google: &GoogleConfig) -> Result<String, String> {
    google
        .client_id
        .clone()
        .or_else(|| option_env!("GOOGLE_OAUTH_CLIENT_ID").map(str::to_string))
        .or_else(|| std::env::var("GOOGLE_OAUTH_CLIENT_ID").ok())
        .ok_or_else(|| "O app ainda nao tem Google OAuth Client ID embutido.".to_string())
}

fn configured_google_client_secret() -> Option<String> {
    option_env!("GOOGLE_OAUTH_CLIENT_SECRET")
        .map(str::to_string)
        .or_else(|| std::env::var("GOOGLE_OAUTH_CLIENT_SECRET").ok())
        .filter(|secret| !secret.trim().is_empty())
}

fn default_minecraft_dir() -> Option<PathBuf> {
    dirs::data_dir()
        .map(|path| path.join(".minecraft"))
        .filter(|path| path.exists())
}

fn default_drive_backup_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let candidates = [
        home.join("Google Drive").join("Mine AutoBackup"),
        home.join("My Drive").join("Mine AutoBackup"),
        home.join("Meu Drive").join("Mine AutoBackup"),
    ];

    candidates
        .into_iter()
        .find(|path| path.parent().map(|parent| parent.exists()).unwrap_or(false))
}

fn default_staging_backup_dir() -> PathBuf {
    config_path()
        .parent()
        .map(|path| path.join("staging"))
        .unwrap_or_else(|| PathBuf::from("Mine AutoBackup").join("staging"))
}
