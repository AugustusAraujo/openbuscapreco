mod admin;
mod config;
mod daemon;
mod db;
mod protocol;
mod tui;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "buscapreco",
    version,
    about = "Port do servidor Gertec Busca Preço (ServUni)"
)]
struct Cli {
    /// Caminho do arquivo de configuração (TOML)
    #[arg(short, long, global = true, default_value = "config.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Roda o daemon (servidor TCP do protocolo + canal admin)
    Serve,
    /// Abre a TUI de configuração/administração
    Tui,
    /// Cria um config.toml de exemplo
    Init,
    /// Verifica se o daemon está aceitando conexões (usado pelo Docker)
    Healthcheck {
        #[arg(default_value = "6500")]
        port: u16,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve => {
            let cfg = match config::Config::load(&cli.config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!(
                        "[erro] ao ler {}: {e}\nuse `buscapreco init` para criar um exemplo",
                        cli.config.display()
                    );
                    std::process::exit(1);
                }
            };
            let mut cfg = cfg;
            if let Ok(url) = std::env::var("DATABASE_URL") {
                if !url.is_empty() {
                    cfg.database.url = url;
                }
            }
            let state = daemon::State::new(cfg, cli.config);
            if let Err(e) = daemon::run(state) {
                eprintln!("[erro] daemon: {e}");
                std::process::exit(1);
            }
        }
        Command::Tui => {
            if let Err(e) = tui::run(&cli.config) {
                eprintln!("[erro] tui: {e}");
                std::process::exit(1);
            }
        }
        Command::Init => {
            if cli.config.exists() {
                eprintln!("{} ja existe", cli.config.display());
                std::process::exit(1);
            }
            let cfg = config::Config::default();
            cfg.save(&cli.config).expect("ao salvar config");
            println!(
                "criado {} — edite [database] e rode `buscapreco serve`",
                cli.config.display()
            );
        }
        Command::Healthcheck { port } => match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Ok(_) => println!("ok"),
            Err(e) => {
                eprintln!("falha: {e}");
                std::process::exit(1);
            }
        },
    }
}
