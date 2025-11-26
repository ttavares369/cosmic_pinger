use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Application, Command, Element, Length, Settings, Theme};
use iced::window;
use ksni::{Tray, MenuItem, ToolTip};
use ksni::menu::StandardItem;
use notify_rust::{Notification, Urgency};
use reqwest::{blocking::Client, StatusCode};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::process::{self, Command as SysCommand};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_NAME: &str = "Cosmic Pinger";

// Monitoring settings
const MONITOR_INTERVAL_SECS: u64 = 180;
const PING_ATTEMPTS: u8 = 3;
const PING_RETRY_DELAY_MS: u64 = 500;
const HTTP_TIMEOUT_SECS: u64 = 5;
const FAIL_STREAK_THRESHOLD: u8 = 2;
const NOTIFICATION_TIMEOUT_MS: i32 = 5000;

// --- CONFIGURAÇÃO ---
#[derive(Serialize, Deserialize, Clone)]
struct AppConfig {
    targets: Vec<String>,
}

impl AppConfig {
    fn default() -> Self {
        Self {
            targets: vec!["google.com".to_string(), "1.1.1.1".to_string()],
        }
    }
}

fn get_config_path() -> PathBuf {
    let dirs = directories::ProjectDirs::from("com", "cosmicpinger", "cosmic_pinger")
        .expect("Não foi possível determinar o diretório de configuração");
    let path = dirs.config_dir();
    if let Err(e) = fs::create_dir_all(path) {
        eprintln!("Erro ao criar diretório de configuração: {}", e);
    }
    path.join("sites.json")
}

fn load_config() -> AppConfig {
    let path = get_config_path();
    if let Ok(content) = fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or(AppConfig::default())
    } else {
        AppConfig::default()
    }
}

fn save_config(cfg: &AppConfig) {
    let path = get_config_path();
    match serde_json::to_string_pretty(cfg) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                eprintln!("Erro ao salvar configuração: {}", e);
            } else {
                println!("Configuração salva em: {:?}", path);
            }
        }
        Err(e) => eprintln!("Erro ao serializar configuração: {}", e),
    }
}

fn normalize_target(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// --- MAIN ---
fn main() {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() > 1 && args[1] == "--config" {
        let settings = Settings {
            window: iced::window::Settings {
                size: iced::Size::new(400.0, 500.0),
                ..Default::default()
            },
            ..Default::default()
        };
        ConfigWindow::run(settings).unwrap();
    } else {
        run_tray();
    }
}

// --- TRAY (BANDEJA) ---
struct PingerState {
    results: Vec<(String, bool, String)>,
    last_update_text: String,
    update_counter: u64,
    all_up: bool,
    first_run: bool,
    fail_streaks: HashMap<String, u8>,
}

fn run_tray() {
    println!("--- Iniciando Modo Tray ---");
    
    let state = Arc::new(Mutex::new(PingerState {
        results: vec![],
        last_update_text: "Aguardando...".to_string(),
        update_counter: 0,
        all_up: true,
        first_run: true,
        fail_streaks: HashMap::new(),
    }));

    let http_client = Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .user_agent(format!("CosmicPinger/{}", APP_VERSION))
        .build()
        .map_err(|err| {
            eprintln!("Falha ao criar cliente HTTP: {}", err);
            err
        })
        .ok();
    let monitor_interval = Duration::from_secs(MONITOR_INTERVAL_SECS);

    // Cria o serviço de tray uma única vez
    let service_state = state.clone();
    let service = ksni::TrayService::new(PingerTray { state: service_state });
    let handle = service.handle();
    service.spawn();
    println!("[TRAY] Serviço de tray iniciado");
    
    let monitor_state = state.clone();
    
    loop {
        let cycle_start = Instant::now();
        let config = load_config();
        let targets = config.targets;
        let client_ref = http_client.as_ref();
        
        let mut raw_results = Vec::new();

        if targets.is_empty() {
             raw_results.push(("Nenhum site configurado".to_string(), true, "-".to_string()));
        } else {
            for target in targets {
                if let Some(cleaned) = normalize_target(&target) {
                    let (success, msg) = check_target(&cleaned, client_ref);
                    raw_results.push((cleaned, success, msg));
                }
            }
            if raw_results.is_empty() {
                raw_results.push(("Nenhum site válido".to_string(), true, "-".to_string()));
            }
        }

        let mut notifications = Vec::new();
        let mut derived_all_up = true;

        {
            let mut s = match monitor_state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let mut fail_map = s.fail_streaks.clone();
            let previous_results = s.results.clone();
            let mut final_results = Vec::with_capacity(raw_results.len());

            for (host, success, msg) in raw_results {
                let entry = fail_map.entry(host.clone()).or_insert(0);
                let (effective_success, display_msg) = if success {
                    *entry = 0;
                    (true, msg)
                } else {
                    *entry = entry.saturating_add(1);
                    if *entry >= FAIL_STREAK_THRESHOLD {
                        (false, msg)
                    } else {
                        let label = format!(
                            "{} (falha {}/{})",
                            msg,
                            *entry,
                            FAIL_STREAK_THRESHOLD
                        );
                        (true, label)
                    }
                };

                if !effective_success {
                    derived_all_up = false;
                }

                final_results.push((host.clone(), effective_success, display_msg));

                if !s.first_run {
                    let previous = previous_results
                        .iter()
                        .find(|(prev_host, _, _)| prev_host == &host)
                        .map(|(_, prev_up, _)| *prev_up);
                    if previous.map(|p| p != effective_success).unwrap_or(true) {
                        notifications.push((host.clone(), effective_success));
                    }
                }
            }

            let valid_hosts: HashSet<String> = final_results.iter().map(|(host, _, _)| host.clone()).collect();
            fail_map.retain(|host, _| valid_hosts.contains(host));

            s.results = final_results;
            s.fail_streaks = fail_map;
            s.update_counter += 1;
            let now = Local::now();
            s.last_update_text = now.format("%H:%M:%S").to_string();
            s.all_up = derived_all_up;
            s.first_run = false;
            
            println!("[CICLO #{}] Checagem concluída às {}. All up: {}", 
                s.update_counter, 
                s.last_update_text,
                s.all_up
            );
        }

        // Notifica o ksni que houve mudança no estado
        handle.update(|_tray| {});

        for (host, is_up) in notifications {
            send_status_notification(&host, is_up);
        }

        let elapsed = cycle_start.elapsed();
        println!("[CICLO] Tempo de execução: {:?}. Dormindo por {:?}", elapsed, monitor_interval.saturating_sub(elapsed));
        let sleep_for = monitor_interval.saturating_sub(elapsed);
        if !sleep_for.is_zero() {
            thread::sleep(sleep_for);
        }
    }
}

fn do_ping(host: &str) -> (bool, String) {
    let mut last_message = "OFFLINE".to_string();

    for attempt in 0..PING_ATTEMPTS {
        let output = SysCommand::new("ping")
            .arg("-c").arg("1")
            .arg("-W").arg("1")
            .arg(host)
            .output();

        match output {
            Ok(out) => {
                if out.status.success() {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if let Some(pos) = stdout.find("time=") {
                        let slice = &stdout[pos + 5..];
                        if let Some((latency, _)) = slice.split_once(" ms") {
                            return (true, format!("{} ms", latency.trim()));
                        }
                    }
                    return (true, "OK".to_string());
                } else {
                    last_message = "OFFLINE".to_string();
                }
            }
            Err(_) => last_message = "Erro".to_string(),
        }

        if attempt + 1 < PING_ATTEMPTS {
            thread::sleep(Duration::from_millis(PING_RETRY_DELAY_MS));
        }
    }

    (false, last_message)
}

fn check_target(target: &str, http_client: Option<&Client>) -> (bool, String) {
    if target.starts_with("http://") || target.starts_with("https://") {
        if let Some(client) = http_client {
            return do_http_check(client, target);
        } else {
            return (false, "HTTP indisponível".to_string());
        }
    }

    do_ping(target)
}

fn do_http_check(client: &Client, url: &str) -> (bool, String) {
    match client.head(url).send() {
        Ok(resp) => {
            let status = resp.status();
            if status == StatusCode::METHOD_NOT_ALLOWED {
                return fetch_via_get(client, url);
            }
            return summarize_http_status(status);
        }
        Err(err) => {
            if err.is_timeout() {
                return (false, "HTTP timeout".to_string());
            }
            eprintln!("HEAD falhou para {}: {}", url, err);
            return fetch_via_get(client, url);
        }
    }
}

fn fetch_via_get(client: &Client, url: &str) -> (bool, String) {
    match client.get(url).send() {
        Ok(resp) => summarize_http_status(resp.status()),
        Err(err) => {
            if err.is_timeout() {
                (false, "HTTP timeout".to_string())
            } else {
                eprintln!("GET falhou para {}: {}", url, err);
                (false, "HTTP erro".to_string())
            }
        }
    }
}

fn summarize_http_status(status: StatusCode) -> (bool, String) {
    let label = format!("HTTP {}", status.as_u16());
    let ok = status.is_success() || status.is_redirection();
    (ok, label)
}

fn send_status_notification(host: &str, is_up: bool) {
    let (summary, body, icon, urgency) = if is_up {
        (
            APP_NAME,
            format!("✅ {} voltou a responder.", host),
            "network-transmit-receive",
            Urgency::Normal,
        )
    } else {
        (
            APP_NAME,
            format!("❌ {} ficou OFFLINE!", host),
            "network-error",
            Urgency::Critical,
        )
    };

    if let Err(e) = Notification::new()
        .summary(summary)
        .body(&body)
        .icon(icon)
        .urgency(urgency)
        .timeout(NOTIFICATION_TIMEOUT_MS)
        .show()
    {
        eprintln!("Erro ao enviar notificação: {}", e);
    }
}

struct PingerTray { state: Arc<Mutex<PingerState>> }

impl Tray for PingerTray {
    fn id(&self) -> String {
        "cosmic-pinger".to_string()
    }

    fn title(&self) -> String {
        APP_NAME.to_string()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let s = match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        
        // Byte 0 = Alpha (255 = Visível)
        // Byte 1 = Red
        // Byte 2 = Green
        // Byte 3 = Blue
        
        let (r, g, b) = if s.first_run { 
            (255, 255, 0) // Amarelo
        } else if s.all_up { 
            (0, 255, 0)   // Verde
        } else { 
            (255, 0, 0)   // Vermelho
        };
        
        let mut data = Vec::with_capacity(32 * 32 * 4);
        for _ in 0..(32 * 32) {
            data.push(255); // A
            data.push(r);   // R
            data.push(g);   // G
            data.push(b);   // B
        }
        
        vec![ksni::Icon { width: 32, height: 32, data }]
    }

    fn tool_tip(&self) -> ToolTip {
        let s = match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let status_txt = if s.first_run { 
            "Iniciando...".to_string()
        } else if s.all_up { 
            format!("Online - {} sites monitorados", s.results.len())
        } else { 
            "⚠️ OFFLINE DETECTADO".to_string()
        };
        
        ToolTip {
            title: format!("{} v{}", APP_NAME, APP_VERSION),
            description: status_txt,
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let s = match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(), // Recupera de mutex poisoned
        };
        let mut items = Vec::new();

        // Usa o timestamp armazenado - simples e estável
        let update_label = format!("Última checagem: {}", s.last_update_text);

        items.push(MenuItem::Standard(StandardItem {
            label: update_label,
            enabled: false,
            ..Default::default()
        }));
        items.push(MenuItem::Separator);

        for (host, is_up, lat) in &s.results {
            items.push(MenuItem::Standard(StandardItem {
                label: format!("{} {} ({})", if *is_up {"🟢"} else {"🔴"}, host, lat),
                enabled: false,
                ..Default::default()
            }));
        }

        items.push(MenuItem::Separator);
        
        items.push(MenuItem::Standard(StandardItem {
            label: "⚙️ Configurar Sites".into(),
            activate: Box::new(|_| {
                if let Ok(exe) = std::env::current_exe() {
                    std::thread::spawn(move || {
                        let _ = SysCommand::new(exe).arg("--config").spawn();
                    });
                }
            }),
            ..Default::default()
        }));

        items.push(MenuItem::Standard(StandardItem {
            label: "Sair".into(),
            activate: Box::new(|_| process::exit(0)),
            ..Default::default()
        }));

        items
    }
}

// --- CONFIG WINDOW (ICED) ---
struct ConfigWindow {
    config: AppConfig,
    input_value: String,
}

#[derive(Debug, Clone)]
enum Message {
    InputChanged(String),
    AddSite,
    RemoveSite(usize),
    SaveAndClose,
}

impl Application for ConfigWindow {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        (ConfigWindow {
            config: load_config(),
            input_value: String::new(),
        }, Command::none())
    }

    fn title(&self) -> String { String::from("Configuração") }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::InputChanged(val) => {
                self.input_value = val;
            },
            Message::AddSite => {
                let trimmed = self.input_value.trim();
                println!("==> AddSite acionado. Valor: '{}'", trimmed);
                if let Some(cleaned) = normalize_target(trimmed) {
                    println!("==> Adicionando site limpo: '{}'", cleaned);
                    self.config.targets.push(cleaned);
                    self.input_value.clear();
                    save_config(&self.config);
                    println!("==> Site adicionado com sucesso. Total: {}", self.config.targets.len());
                } else {
                    println!("==> Valor vazio ou inválido, não adicionando");
                }
            },
            Message::RemoveSite(idx) => {
                if idx < self.config.targets.len() {
                    let removed = self.config.targets.remove(idx);
                    println!("==> Removido site: {}", removed);
                    save_config(&self.config);
                }
            },
            Message::SaveAndClose => {
                println!("==> SaveAndClose acionado");
                save_config(&self.config);
                return window::close(window::Id::MAIN);
            }
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let input_row = row![
            text_input("Ex: google.com", &self.input_value)
                .on_input(Message::InputChanged)
                .on_submit(Message::AddSite)
                .padding(10)
                .width(Length::Fill),
            button(" + Adicionar ").on_press(Message::AddSite).padding(10)
        ].spacing(10);

        let mut list_col = column![].spacing(10);
        
        let count_text = text(format!("Sites monitorados: {}", self.config.targets.len())).size(14);

        for (i, site) in self.config.targets.iter().enumerate() {
            list_col = list_col.push(
                container(
                    row![
                        text(site).width(Length::Fill).size(16),
                        button(" Remover ").on_press(Message::RemoveSite(i)).style(iced::theme::Button::Destructive)
                    ].align_items(iced::Alignment::Center)
                )
                .padding(10)
                .style(iced::theme::Container::Box)
            );
        }

        let content = column![
            text("Monitoramento").size(26),
            input_row,
            count_text,
            scrollable(list_col).height(Length::Fill),
            button("Salvar e Fechar").on_press(Message::SaveAndClose).padding(15).width(Length::Fill)
        ].spacing(20).padding(20);

        container(content).width(Length::Fill).height(Length::Fill).into()
    }
}