use std::path::PathBuf;
use std::thread::JoinHandle;

use mea::mpsc::BoundedReceiver;
use ntex::web;
use rand::distr::Alphanumeric;
use rand::RngExt;

use simeis_data::config::init_config;
use simeis_data::game::{Game, GameSignal};
use simeis_data::syslog::SyslogRecv;

mod api;
mod utils;

pub type GameState = ntex::web::types::State<Game>;
pub type AdminToken = ntex::web::types::State<String>;

pub fn generate_random_token(len: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

// #[cfg(feature = "compio")]
// fn start_game_thread(
//     stop: BoundedReceiver<GameSignal>,
//     sysrecv: SyslogRecv,
//     data: Game,
// ) -> JoinHandle<()> {
//     log::debug!("Starting the game thread on compio runtime");
//     std::thread::spawn(move || {
//         let rt = compio::runtime::Runtime::new().unwrap();
//         rt.block_on(data.start(stop, sysrecv, compio::runtime::time::sleep));
//         rt.run();
//     })
// }

#[cfg(feature = "tokio")]
fn start_game_thread(
    stop: BoundedReceiver<GameSignal>,
    sysrecv: SyslogRecv,
    data: Game,
) -> JoinHandle<()> {
    log::debug!("Starting the game thread on tokio runtime");
    let rt =
        tokio::runtime::Runtime::new().expect("Unable to create new tokio runtime for game thread");
    std::thread::spawn(move || rt.block_on(data.start(stop, sysrecv, tokio::time::sleep)))
}

// Simeis is a game player with an API
// To play, you must start by creating a player with `/player/new/{name}`
// The key you get from this API must be passed to each request as a HTTP header of key "Simeis-Key"
#[ntex::main]
async fn main() -> std::io::Result<()> {
    let mut args = std::env::args();
    args.next();

    let Some(cfg_file) = args.next() else {
        println!("Missing argument: configuration file");
        return Err(std::io::ErrorKind::InvalidData.into());
    };
    let cfg_file = PathBuf::from(cfg_file);

    let config = init_config(&cfg_file).expect("Unable to initialize configuration");
    config.validation_tests();
    let port = config.server.port;

    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_env(&config.server.logger_env_name)
        .filter_module("ntex_server", log::LevelFilter::Warn)
        .filter_module("ntex_io", log::LevelFilter::Warn)
        .filter_module("ntex_rt", log::LevelFilter::Warn)
        .filter_module("ntex_service::cfg", log::LevelFilter::Warn)
        .filter_module("ntex::http::h1", log::LevelFilter::Warn)
        // .filter_module("ntex_net::compio", log::LevelFilter::Warn)
        .filter_module("ntex_net::tokio", log::LevelFilter::Warn)
        .init();

    log::info!(
        "Running {} workers on http://{}:{port}",
        config.server.workers,
        config.server.bind_ip,
    );
    // TODO (#34) Reduce stack size from this task, > 1024
    let (gamethread, state) = Game::init(start_game_thread).await;
    let stop_chan = state.send_sig.clone();

    let admin_token =
        std::env::var("SIMEIS_ADMIN_TOKEN").unwrap_or_else(|_| generate_random_token(32));

    #[cfg(not(feature = "testing"))]
    {
        std::fs::write(config.server.token_file.clone(), &admin_token)?;
        println!("SIMEIS_ADMIN_TOKEN={admin_token}");
    }

    let res = web::HttpServer::new(async move || {
        let game_state = state.clone();
        web::App::new()
            .middleware(web::middleware::Logger::default())
            .state(game_state)
            .state(admin_token.clone())
            .configure(api::configure)
    })
    .workers(config.server.workers)
    .bind((config.server.bind_ip.as_str(), port))?
    .run()
    .await;

    log::info!("Server stopped, stopping game thread");
    stop_chan.send(GameSignal::Stop).await.unwrap();
    gamethread.join().unwrap();
    res
}
