//! Renderização (ratatui).

use super::app::{self, App, Tab};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::Frame;

const TITULOS: [&str; 4] = ["1 Config", "2 Terminais", "3 Preco", "4 Log"];

pub fn draw(f: &mut Frame, app: &mut App) {
    let [top, mid, bot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .areas(f.area());

    let idx = match app.tab {
        Tab::Config => 0,
        Tab::Terminais => 1,
        Tab::Preco => 2,
        Tab::Log => 3,
    };
    f.render_widget(
        Tabs::new(TITULOS).select(idx).highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        top,
    );

    match app.tab {
        Tab::Config => draw_config(f, app, mid),
        Tab::Terminais => draw_terminals(f, app, mid),
        Tab::Preco => draw_price(f, app, mid),
        Tab::Log => draw_log(f, app, mid),
    }

    let daemon = if app.daemon_on {
        Span::styled("daemon: conectado", Style::default().fg(Color::Green))
    } else {
        Span::styled("daemon: desconectado", Style::default().fg(Color::Red))
    };
    let ajuda = match app.tab {
        Tab::Config => " ↑↓ move | ←→ cursor | digita | Enter aplica e salva | q sai",
        Tab::Terminais => {
            " ↑↓ seleciona | m mensagem | Enter mensagem p/ selecionado | r restart todos | q sai"
        }
        Tab::Preco => " digita codigo | Enter consulta | q sai",
        Tab::Log => " PageUp/PageDown rola | q sai",
    };
    f.render_widget(Paragraph::new(Line::from(vec![daemon])), bot);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {}| {}", app.status, ajuda.trim_start()),
            Style::default().fg(Color::Gray),
        )))
        .wrap(Wrap { trim: true }),
        Rect {
            y: bot.y + 1,
            height: 1,
            ..bot
        },
    );
}

fn draw_config(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = (0..app::FIELDS.len())
        .map(|i| {
            let (label, _) = app::FIELDS[i];
            let value = app::field_value(&app.edit, i);
            let line: Line = if i == app.selected {
                let at = app.cursor.min(value.len());
                let mut s = format!("{label:<18} {}", {
                    let mut v = value.clone();
                    v.insert(at, '|');
                    v
                });
                s.insert_str(0, "> ");
                Line::from(Span::styled(s, Style::default().fg(Color::Yellow)))
            } else {
                Line::from(format!("  {label:<18} {value}"))
            };
            ListItem::new(line)
        })
        .collect();
    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" config.toml "),
        ),
        area,
    );
}

fn draw_terminals(f: &mut Frame, app: &App, area: Rect) {
    let rows = app.terminals.iter().enumerate().map(|(i, t)| {
        let cells = [
            t.id.clone(),
            t.model.clone(),
            t.firmware.clone(),
            t.addr.clone(),
            t.connected_since.clone(),
            t.queries.to_string(),
        ];
        let style = if i == app.term_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        Row::new(cells).style(style)
    });
    let tabela = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(14),
            Constraint::Length(12),
            Constraint::Length(22),
            Constraint::Length(19),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("ID"),
            Cell::from("Modelo"),
            Cell::from("Firmware"),
            Cell::from("Endereco"),
            Cell::from("Conectado ha"),
            Cell::from("Consultas"),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL).title(" Terminais "));
    f.render_widget(tabela, area);

    if let Some(prompt) = &app.msg {
        let h = 5;
        let popup = Rect {
            x: area.x + 1,
            y: area.bottom().saturating_sub(h + 1),
            width: area.width.saturating_sub(2),
            height: h.min(area.height),
        };
        let dest = prompt.target.clone().unwrap_or_else(|| "todos".into());
        let rotulos = ["linha1", "linha2", "segundos"];
        let linhas: Vec<Line> = (0..3)
            .map(|i| {
                let marca = if i == prompt.idx { "> " } else { "  " };
                let valor = if i == prompt.idx {
                    format!("{}_", prompt.fields[i])
                } else {
                    prompt.fields[i].clone()
                };
                Line::from(format!("{marca}{}: {valor}", rotulos[i]))
            })
            .collect();
        let mut conteudo = vec![Line::from(format!("mensagem para: {dest}"))];
        conteudo.extend(linhas);
        conteudo.push(Line::from(
            "Enter avanca/envia | Tab troca campo | Esc cancela",
        ));
        f.render_widget(
            Paragraph::new(conteudo)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Enviar mensagem "),
                )
                .style(Style::default().fg(Color::Cyan)),
            popup,
        );
    }
}

fn draw_price(f: &mut Frame, app: &App, area: Rect) {
    let [entrada, historico] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).areas(area);
    f.render_widget(
        Paragraph::new(format!("{}_", app.price_input)).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Codigo de barras "),
        ),
        entrada,
    );
    let items: Vec<ListItem> = app
        .price_history
        .iter()
        .map(|l| ListItem::new(l.clone()))
        .collect();
    f.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(" Historico ")),
        historico,
    );
}

fn draw_log(f: &mut Frame, app: &mut App, area: Rect) {
    app.log_viewport = area.height.saturating_sub(2);
    let linhas: Vec<Line> = app
        .log_lines
        .iter()
        .map(|l| Line::from(l.clone()))
        .collect();
    f.render_widget(
        Paragraph::new(linhas)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Log (ultimas 100) "),
            )
            .scroll((app.log_scroll, 0)),
        area,
    );
}
