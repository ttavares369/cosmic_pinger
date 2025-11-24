use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Application, Command, Element, Length, Settings, Theme};
use ksni::{Tray, MenuItem, ToolTip};
use ksni::menu::StandardItem;
use notify_rust::{Notification, Urgency};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::process::{self, Command as SysCommand};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const MONITOR_INTERVAL_SECS: u64 = 180;

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
    let dirs = directories::ProjectDirs::from("com", "tiago", "cosmic_pinger").unwrap();
    let path = dirs.config_dir();
    fs::create_dir_all(path).unwrap();
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
    let json = serde_json::to_string_pretty(cfg).unwrap();
    // A CORREÇÃO ESTÁ AQUI: Usamos &path para "emprestar" o valor, não mover.
    fs::write(&path, json).unwrap();
    println!("Configuração salva em: {:?}", path);
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
    last_update: String,
    all_up: bool,
    first_run: bool,
}

fn run_tray() {
    println!("--- Iniciando Modo Tray (ARGB Final) ---");
    
    let state = Arc::new(Mutex::new(PingerState {
        results: vec![],
        last_update: "Aguardando...".to_string(),
        all_up: true,
        first_run: true,
    }));

    let service_state = state.clone();
    let service = ksni::TrayService::new(PingerTray { state: service_state });
    let handle = service.handle();
    service.spawn();

    let monitor_state = state.clone();
    
    loop {
        let config = load_config();
        let targets = config.targets;
        
        let mut temp_results = Vec::new();
        let mut all_ok = true;

        if targets.is_empty() {
             temp_results.push(("Nenhum site configurado".to_string(), true, "-".to_string()));
        } else {
            for target in targets {
                let (success, msg) = do_ping(&target);
                if !success { all_ok = false; }
                temp_results.push((target, success, msg));
            }
        }

        let mut notifications = Vec::new();

        {
            let mut s = monitor_state.lock().unwrap();

            if !s.first_run {
                for (host, is_up, _) in &temp_results {
                    let previous = s.results.iter().find(|(prev_host, _, _)| prev_host == host).map(|(_, prev_up, _)| *prev_up);
                    if previous.map(|p| p != *is_up).unwrap_or(true) {
                        notifications.push((host.clone(), *is_up));
                    }
                }
            }

            s.results = temp_results;
            s.last_update = Local::now().format("%d/%m/%Y %H:%M:%S").to_string();
            s.all_up = all_ok;
            s.first_run = false;
        }

        handle.update(|_| {});
        for (host, is_up) in notifications {
            send_status_notification(&host, is_up);
        }
        thread::sleep(Duration::from_secs(MONITOR_INTERVAL_SECS));
    }
}

fn do_ping(host: &str) -> (bool, String) {
    let output = SysCommand::new("ping").arg("-c").arg("1").arg("-W").arg("1").arg(host).output();
    match output {
        Ok(out) => {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Some(pos) = stdout.find("time=") {
                    let slice = &stdout[pos + 5..];
                    if let Some((latency, _)) = slice.split_once(" ms") {
                        (true, format!("{} ms", latency.trim()))
                    } else {
                        (true, "OK".to_string())
                    }
                } else { (true, "OK".to_string()) }
            } else { (false, "OFFLINE".to_string()) }
        },
        Err(_) => (false, "Erro".to_string()),
    }
}

fn send_status_notification(host: &str, is_up: bool) {
    let (summary, body, icon, urgency) = if is_up {
        (
            "Cosmic Pinger",
            format!("{} voltou a responder.", host),
            "dialog-information",
            Urgency::Normal,
        )
    } else {
        (
            "Cosmic Pinger",
            format!("{} ficou OFFLINE.", host),
            "dialog-warning",
            Urgency::Critical,
        )
    };

    let _ = Notification::new()
        .summary(summary)
        .body(&body)
        .icon(icon)
        .urgency(urgency)
        .show();
}

struct PingerTray { state: Arc<Mutex<PingerState>> }

impl Tray for PingerTray {
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let s = self.state.lock().unwrap();
        
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
        let s = self.state.lock().unwrap();
        let status_txt = if s.first_run { "Iniciando..." } 
                         else if s.all_up { "Online" } 
                         else { "OFFLINE DETECTADO" };
        
        ToolTip {
            title: "Cosmic Pinger".to_string(),
            description: status_txt.to_string(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let s = self.state.lock().unwrap();
        let mut items = Vec::new();

        items.push(MenuItem::Standard(StandardItem {
            label: format!("Update: {}", s.last_update),
            enabled: false,
            ..Default::default()
        }));
        items.push(MenuItem::Separator);

        for (host, is_up, lat) in &s.results {
            items.push(MenuItem::Standard(StandardItem {
                label: format!("{} {} ({})", if *is_up {"🟢"} else {"🔴"}, host, lat),
                ..Default::default()
            }));
        }

        items.push(MenuItem::Separator);
        
        items.push(MenuItem::Standard(StandardItem {
            label: "⚙️ Configurar Sites".into(),
            activate: Box::new(|_| {
                let exe = std::env::current_exe().unwrap();
                std::thread::spawn(move || {
                    SysCommand::new(exe).arg("--config").spawn().unwrap();
                });
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
            Message::InputChanged(val) => self.input_value = val,
            Message::AddSite => {
                println!("Tentando adicionar site: '{}'", self.input_value);
                if !self.input_value.trim().is_empty() {
                    self.config.targets.push(self.input_value.trim().to_string());
                    self.input_value.clear();
                }
            },
            Message::RemoveSite(idx) => { self.config.targets.remove(idx); },
            Message::SaveAndClose => {
                save_config(&self.config);
                std::process::exit(0);
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