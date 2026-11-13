//! TUI de configuração/administração (ratatui + crossterm).

mod app;
mod client;
mod ui;

use crate::config::Config;
use app::{App, MsgPrompt, Tab};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::time::{Duration, Instant};

pub fn run(config_path: &std::path::Path) -> std::io::Result<()> {
    let config = Config::load(config_path).unwrap_or_default();
    let mut terminal = ratatui::init();
    let res = event_loop(&mut terminal, config, config_path);
    ratatui::restore();
    res
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    config: Config,
    config_path: &std::path::Path,
) -> std::io::Result<()> {
    let mut app = App::new(config, config_path.to_path_buf());
    refresh(&mut app, true);
    loop {
        terminal.draw(|f| ui::draw(f, &mut app))?;
        if crossterm::event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = crossterm::event::read()? {
                if key.kind == KeyEventKind::Press && handle_key(&mut app, key) {
                    return Ok(());
                }
            }
        }
        refresh(&mut app, false);
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if app.msg.is_some() {
        return handle_msg_key(app, key);
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Char('1') => app.tab = Tab::Config,
        KeyCode::Char('2') => app.tab = Tab::Terminais,
        KeyCode::Char('3') => app.tab = Tab::Preco,
        KeyCode::Char('4') => app.tab = Tab::Log,
        KeyCode::Tab => app.tab = app.tab.next(),
        _ => match app.tab {
            Tab::Config => handle_config_key(app, key),
            Tab::Terminais => handle_term_key(app, key),
            Tab::Preco => handle_price_key(app, key),
            Tab::Log => handle_log_key(app, key),
        },
    }
    false
}

fn handle_config_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            app.selected = app.selected.saturating_sub(1);
            app.cursor = app::field_value(&app.edit, app.selected).len();
        }
        KeyCode::Down => {
            if app.selected + 1 < app::FIELDS.len() {
                app.selected += 1;
                app.cursor = app::field_value(&app.edit, app.selected).len();
            }
        }
        KeyCode::Left => app.cursor = app.cursor.saturating_sub(1),
        KeyCode::Right => {
            let len = app::field_value(&app.edit, app.selected).len();
            if app.cursor < len {
                app.cursor += 1;
            }
        }
        KeyCode::Backspace => {
            if app.cursor > 0 {
                let i = app.selected;
                let mut v = app::field_value(&app.edit, i);
                app.cursor -= 1;
                v.remove(app.cursor);
                app::set_field_value(&mut app.edit, i, &v);
            }
        }
        KeyCode::Char(c) => {
            let i = app.selected;
            if app::FIELDS[i].1 && !c.is_ascii_digit() {
                return;
            }
            let mut v = app::field_value(&app.edit, i);
            let at = app.cursor.min(v.len());
            v.insert(at, c);
            app.cursor = at + 1;
            app::set_field_value(&mut app.edit, i, &v);
        }
        KeyCode::Enter => save_config(app),
        _ => {}
    }
}

fn save_config(app: &mut App) {
    for i in 0..app::FIELDS.len() {
        if app::FIELDS[i].1 && app::field_value(&app.edit, i).parse::<u64>().is_err() {
            app.status = format!("valor invalido em {} — campo numerico", app::FIELDS[i].0);
            return;
        }
    }
    match client::request(
        app.daemon_port(),
        &crate::admin::AdminRequest::SetConfig {
            config: app.edit.clone(),
        },
    ) {
        Ok(crate::admin::AdminResponse::Success { .. }) => {
            if let Err(e) = app.edit.save(&app.config_path) {
                app.status = format!("daemon aplicou config, mas falha ao salvar local: {e}");
            } else {
                app.status = "config aplicada ao daemon e salva".into();
            }
        }
        Ok(crate::admin::AdminResponse::Error { message }) => {
            app.status = format!("daemon recusou config ({message}); nada salvo");
        }
        Err(e) => match app.edit.save(&app.config_path) {
            Ok(()) => app.status = format!("daemon indisponivel ({e}); config salva apenas local"),
            Err(e2) => app.status = format!("falha ao salvar local: {e2}"),
        },
    }
}

fn handle_term_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up => app.term_selected = app.term_selected.saturating_sub(1),
        KeyCode::Down => {
            if !app.terminals.is_empty() && app.term_selected + 1 < app.terminals.len() {
                app.term_selected += 1;
            }
        }
        KeyCode::Char('m') => {
            let target = app
                .terminals
                .get(app.term_selected)
                .map(|t| t.id.clone())
                .or(None);
            app.msg = Some(MsgPrompt::new(target));
        }
        KeyCode::Enter => {
            if let Some(t) = app.terminals.get(app.term_selected) {
                app.msg = Some(MsgPrompt::new(Some(t.id.clone())));
            }
        }
        KeyCode::Char('r') => {
            match client::request(
                app.daemon_port(),
                &crate::admin::AdminRequest::Restart { id: None },
            ) {
                Ok(crate::admin::AdminResponse::Success { data }) => {
                    if let crate::admin::AdminData::Done { detail } = &*data {
                        app.status = format!("restart: {detail}");
                    }
                }
                Ok(crate::admin::AdminResponse::Error { message }) => {
                    app.status = format!("erro no restart: {message}");
                }
                Err(e) => app.status = format!("daemon indisponivel: {e}"),
            }
            app.last_term_refresh = Instant::now();
        }
        _ => {}
    }
}

fn handle_msg_key(app: &mut App, key: KeyEvent) -> bool {
    let Some(prompt) = app.msg.as_mut() else {
        return false;
    };
    match key.code {
        KeyCode::Esc => app.msg = None,
        KeyCode::Up | KeyCode::BackTab => prompt.idx = (prompt.idx + 2) % 3,
        KeyCode::Down | KeyCode::Tab => prompt.idx = (prompt.idx + 1) % 3,
        KeyCode::Backspace => {
            prompt.fields[prompt.idx].pop();
        }
        KeyCode::Char(c) => {
            if prompt.idx != 2 || c.is_ascii_digit() {
                prompt.fields[prompt.idx].push(c);
            }
        }
        KeyCode::Enter => {
            if prompt.idx < 2 {
                prompt.idx += 1;
            } else {
                let secs = prompt.fields[2].parse::<u8>().unwrap_or(5);
                let req = crate::admin::AdminRequest::SendText {
                    id: prompt.target.clone(),
                    line1: prompt.fields[0].clone(),
                    line2: prompt.fields[1].clone(),
                    secs,
                };
                let dest = prompt.target.clone().unwrap_or_else(|| "todos".into());
                app.msg = None;
                match client::request(app.daemon_port(), &req) {
                    Ok(crate::admin::AdminResponse::Success { data }) => {
                        if let crate::admin::AdminData::Done { detail } = &*data {
                            app.status = format!("mensagem para {dest}: {detail}");
                        }
                    }
                    Ok(crate::admin::AdminResponse::Error { message }) => {
                        app.status = format!("erro ao enviar para {dest}: {message}");
                    }
                    Err(e) => app.status = format!("daemon indisponivel: {e}"),
                }
                app.last_term_refresh = Instant::now();
            }
        }
        _ => {}
    }
    false
}

fn handle_price_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Backspace => {
            app.price_input.pop();
        }
        KeyCode::Char(c) if c.is_ascii_digit() => app.price_input.push(c),
        KeyCode::Enter if !app.price_input.is_empty() => {
            let code = app.price_input.clone();
            match client::request(
                app.daemon_port(),
                &crate::admin::AdminRequest::LookupPrice {
                    barcode: code.clone(),
                },
            ) {
                Ok(crate::admin::AdminResponse::Success { data }) => match &*data {
                    crate::admin::AdminData::Price {
                        found, name, price, ..
                    } => {
                        let linha = if *found {
                            format!(
                                "{code} → {} — {}",
                                name.clone().unwrap_or_default(),
                                price.clone().unwrap_or_default()
                            )
                        } else {
                            format!("{code} → nao encontrado")
                        };
                        app.status = linha.clone();
                        app.price_history.insert(0, linha);
                        app.price_history.truncate(20);
                    }
                    _ => app.status = "resposta inesperada do daemon".into(),
                },
                Ok(crate::admin::AdminResponse::Error { message }) => {
                    app.status = format!("erro: {message}");
                }
                Err(e) => app.status = format!("daemon indisponivel: {e}"),
            }
            app.price_input.clear();
        }
        _ => {}
    }
}

fn handle_log_key(app: &mut App, key: KeyEvent) {
    let max = app.log_lines.len().saturating_sub(1) as u16;
    match key.code {
        KeyCode::Up => {
            app.log_follow = false;
            app.log_scroll = app.log_scroll.saturating_sub(1);
        }
        KeyCode::Down => app.log_scroll = (app.log_scroll + 1).min(max),
        KeyCode::PageUp => {
            app.log_follow = false;
            app.log_scroll = app.log_scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            app.log_scroll = (app.log_scroll + 10).min(max);
            if app.log_scroll >= max {
                app.log_follow = true;
            }
        }
        _ => {}
    }
}

fn refresh(app: &mut App, force: bool) {
    match app.tab {
        Tab::Config => {
            if force || app.last_cfg_check.elapsed() >= Duration::from_secs(3) {
                app.last_cfg_check = Instant::now();
                match client::request(app.daemon_port(), &crate::admin::AdminRequest::GetConfig) {
                    Ok(crate::admin::AdminResponse::Success { .. }) => app.daemon_on = true,
                    _ => app.daemon_on = false,
                }
            }
        }
        Tab::Terminais => {
            if force || app.last_term_refresh.elapsed() >= Duration::from_secs(2) {
                app.last_term_refresh = Instant::now();
                if let Ok(crate::admin::AdminResponse::Success { data }) = client::request(
                    app.daemon_port(),
                    &crate::admin::AdminRequest::ListTerminals,
                ) {
                    if let crate::admin::AdminData::Terminals { terminals } = &*data {
                        app.daemon_on = true;
                        if app.term_selected >= terminals.len() {
                            app.term_selected = terminals.len().saturating_sub(1);
                        }
                        app.terminals = terminals.clone();
                    }
                } else {
                    app.daemon_on = false;
                    app.terminals.clear();
                }
            }
        }
        Tab::Log => {
            if force || app.last_log_refresh.elapsed() >= Duration::from_secs(1) {
                app.last_log_refresh = Instant::now();
                if let Ok(crate::admin::AdminResponse::Success { data }) = client::request(
                    app.daemon_port(),
                    &crate::admin::AdminRequest::LogTail { n: 100 },
                ) {
                    if let crate::admin::AdminData::Log { lines } = &*data {
                        app.daemon_on = true;
                        let max = lines.len().saturating_sub(1) as u16;
                        if app.log_follow {
                            app.log_scroll = max;
                        } else {
                            app.log_scroll = app.log_scroll.min(max);
                        }
                        app.log_lines = lines.clone();
                    }
                } else {
                    app.daemon_on = false;
                }
            }
        }
        Tab::Preco => {}
    }
}
