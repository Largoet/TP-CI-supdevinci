// Mutex acquisition order
// - Taken names
// - Player index
// - Player list
// - Station index
// - Ship owners
// - Player
// - Station
// - Galaxy
// - Market
// - SyslogFifo
// - PlayerFifo

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mea::mpsc::{BoundedReceiver, BoundedSender, RecvError};
use mea::rwlock::RwLock;

#[cfg(not(feature = "testing"))]
use mea::mpsc::TryRecvError;

use base64::{prelude::BASE64_STANDARD, Engine};
use rand::{Rng, RngExt};

use crate::config::get_config;
use crate::errors::Errcode;
use crate::galaxy::scan::ScanResult;
use crate::galaxy::station::{Station, StationId};
use crate::galaxy::Galaxy;
use crate::history::PlayerHistoryEvent;
use crate::market::{Market, MarketTx};
use crate::player::{Player, PlayerId, PlayerKey};
use crate::ship::resources::{ExtractionInfo, Resource};
use crate::ship::{ShipId, ShipState};
use crate::syslog::{SyslogEvent, SyslogFifo, SyslogRecv, SyslogSend};
use crate::utils::ShardedLockedData;

#[cfg(not(feature = "extraspeed"))]
const ITER_PERIOD: Duration = Duration::from_millis(20);

#[cfg(feature = "extraspeed")]
const ITER_PERIOD: Duration = Duration::from_micros(20);

// TODO (#9) Have a global "inflation" rate for all users, that increases over time
//     Equipment becomes more and more expansive

pub enum GameSignal {
    Stop,
    Tick,

    #[cfg(feature = "testing")]
    TestTick(Arc<mea::latch::Latch>),
}

#[derive(Clone)]
pub struct Game {
    // Locked
    pub players: Arc<ShardedLockedData<PlayerId, Arc<RwLock<Player>>>>,
    pub player_index: Arc<ShardedLockedData<PlayerKey, PlayerId>>,
    pub taken_names: Arc<ShardedLockedData<String, PlayerId>>,
    pub galaxy: Arc<RwLock<Galaxy>>,

    pub stations: Arc<RwLock<BTreeMap<StationId, Arc<Station>>>>,
    pub init_station: Arc<RwLock<(StationId, Arc<Station>)>>,
    pub market: Arc<Market>,
    pub syslog: SyslogSend,
    pub fifo_events: SyslogFifo,
    pub send_sig: BoundedSender<GameSignal>,

    // Tracking data
    pub ship_owners: Arc<RwLock<BTreeMap<ShipId, PlayerId>>>,
}

impl Game {
    pub async fn empty(
        send_stop: BoundedSender<GameSignal>,
        syslog: SyslogSend,
        fifo_events: SyslogFifo,
    ) -> Game {
        let mut galaxy = Galaxy::init();
        let init_station = galaxy.init_new_station().await;
        let mut stations = BTreeMap::new();
        stations.insert(init_station.0, init_station.1.clone());
        let init_station = Arc::new(RwLock::new(init_station));
        let cfg = get_config();
        Game {
            send_sig: send_stop,
            galaxy: Arc::new(RwLock::new(galaxy)),
            market: Arc::new(Market::init()),
            taken_names: Arc::new(ShardedLockedData::new(cfg.game.shards)),
            players: Arc::new(ShardedLockedData::new(cfg.game.shards)),
            player_index: Arc::new(ShardedLockedData::new(cfg.game.shards)),
            syslog,
            fifo_events,
            stations: Arc::new(RwLock::new(stations)),
            init_station,
            ship_owners: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub async fn init<F>(handle_thread: F) -> (std::thread::JoinHandle<()>, Game)
    where
        F: FnOnce(BoundedReceiver<GameSignal>, SyslogRecv, Game) -> std::thread::JoinHandle<()>,
    {
        let (send_stop, recv_stop) = mea::mpsc::bounded(5);
        let (syssend, sysrecv) = SyslogSend::channel();
        let data = Game::empty(send_stop, syssend, sysrecv.fifo.clone()).await;
        let thread_data = data.clone();
        let thread = handle_thread(recv_stop, sysrecv, thread_data);
        (thread, data)
    }

    #[allow(unused_mut)]
    pub async fn start<F, Fut>(
        &self,
        mut stop: BoundedReceiver<GameSignal>,
        syslog: SyslogRecv,
        slp: F,
    ) where
        Fut: Future,
        F: Fn(Duration) -> Fut,
    {
        log::info!("Game thread started");
        let sleepmin_iter = ITER_PERIOD;
        let mut last_iter = Instant::now();
        let mut market_last_tick = Instant::now();
        let mut rng: rand::rngs::SmallRng = rand::make_rng();

        'main: loop {
            #[cfg(feature = "testing")]
            let got = stop.recv().await;

            #[cfg(not(feature = "testing"))]
            let got = match stop.try_recv() {
                Ok(res) => Ok(res),
                Err(TryRecvError::Empty) => Ok(GameSignal::Tick),
                Err(TryRecvError::Disconnected) => {
                    log::error!("Can't get next tick / stop signal: disconnected");
                    Err(RecvError::Disconnected)
                }
            };

            match got {
                Ok(GameSignal::Tick) => {
                    self.threadloop(&mut rng, &mut market_last_tick, &syslog)
                        .await;
                    let took = Instant::now() - last_iter;
                    let t = sleepmin_iter.saturating_sub(took);
                    slp(t).await;
                    last_iter = Instant::now();
                }

                #[cfg(feature = "testing")]
                Ok(GameSignal::TestTick(b)) => {
                    self.threadloop(&mut rng, &mut market_last_tick, &syslog)
                        .await;
                    b.count_down();
                }

                Ok(GameSignal::Stop) => break 'main,
                Err(RecvError::Disconnected) => {
                    log::error!("Got disconnected channel in game thread");
                    break 'main;
                }
            }
        }
        log::info!("Exiting game thread");
    }

    async fn threadloop<R: Rng>(&self, rng: &mut R, mlt: &mut Instant, syslog: &SyslogRecv) {
        let cfg = get_config();
        let market_change_proba = (mlt.elapsed().as_secs_f64() / cfg.market.change_sec).min(1.0);

        let all_players: Vec<PlayerId> = self.players.get_all_keys().await;
        for player_id in all_players {
            let player = self.players.clone_val(&player_id).await.unwrap();
            let mut player = player.write().await;
            player.update_money(syslog, ITER_PERIOD.as_secs_f64()).await;

            for station in player.stations.values() {
                station
                    .update_crafting(syslog, ITER_PERIOD.as_secs_f64(), &player_id)
                    .await;
            }

            let mut deadship = vec![];
            for (id, ship) in player.ships.iter_mut() {
                match ship.state {
                    ShipState::InFlight(..) => {
                        let finished = ship.update_flight(ITER_PERIOD.as_secs_f64());
                        if finished {
                            ship.state = ShipState::Idle;
                            if ship.hull_decay >= ship.hull_resistance {
                                deadship.push(*id);
                            } else {
                                log::debug!(
                                    "Ship flight finished {id}: fuel {:.2}/{:.2}, hull {:.2}/{:.2}",
                                    ship.fuel_tank,
                                    ship.fuel_tank_capacity,
                                    ship.hull_decay,
                                    ship.hull_resistance
                                );
                                syslog
                                    .event(player_id, SyslogEvent::ShipFlightFinished(*id))
                                    .await;
                            }
                        }
                    }

                    ShipState::Extracting(..) => {
                        let finished = ship.update_extract(ITER_PERIOD.as_secs_f64());
                        if finished {
                            ship.state = ShipState::Idle;
                            syslog
                                .event(player_id, SyslogEvent::ExtractionStopped(*id))
                                .await;
                        }
                    }
                    _ => {}
                }
            }
            for id in deadship {
                syslog
                    .event(player_id, SyslogEvent::ShipDestroyed(id))
                    .await;
                player.ships.remove(&id);
            }
        }

        if rng.random_bool(market_change_proba) {
            #[cfg(not(feature = "testing"))]
            self.market.update_prices(rng).await;
            *mlt = Instant::now();
        }

        syslog.update().await;
    }

    pub async fn new_player(&self, name: String) -> Result<(PlayerId, String), Errcode> {
        if self.taken_names.contains_key(&name).await {
            return Err(Errcode::PlayerAlreadyExists(name));
        }

        let init_station = self.init_station.read().await;
        let player = Player::new(init_station.clone(), name.clone());
        let pid = player.id;
        self.taken_names.insert(name.clone(), pid).await;

        let key = BASE64_STANDARD.encode(player.key);

        self.player_index.insert(player.key, player.id).await;
        self.players
            .insert(player.id, Arc::new(RwLock::new(player)))
            .await;
        self.syslog.event(&pid, SyslogEvent::GameStarted).await;
        Ok((pid, key))
    }

    pub async fn get_player(
        &self,
        key: &PlayerKey,
    ) -> Result<(PlayerId, Arc<RwLock<Player>>), Errcode> {
        let Some(id) = self.player_index.clone_val(key).await else {
            return Err(Errcode::NoPlayerWithKey);
        };
        let player = self.players.clone_val(&id).await.unwrap();
        if player.read().await.lost {
            self.taken_names.remove_value(&id).await;
            self.player_index.remove_value(&id).await;
            return Err(Errcode::PlayerLost);
        }
        Ok((id, player.clone()))
    }

    pub async fn get_syslogs(&self, pkey: &PlayerKey) -> Result<Vec<(f64, SyslogEvent)>, Errcode> {
        let (pid, _) = self.get_player(pkey).await?;
        let allfifo = self.fifo_events.read().await;
        let Some(fifo) = allfifo.clone_val(&pid).await else {
            return Ok(vec![]);
        };
        let mut fifo = fifo.write().await;
        Ok(fifo.remove_all())
    }

    pub async fn scan_galaxy(
        &self,
        pkey: &PlayerKey,
        station_id: &StationId,
    ) -> Result<ScanResult, Errcode> {
        let (_, player) = self.get_player(pkey).await?;
        let player = player.read().await;
        let galaxy = self.galaxy.read().await;
        let Some(station) = player.stations.get(station_id) else {
            return Err(Errcode::NoSuchStation(*station_id));
        };
        Ok(station.scan(&galaxy))
    }

    pub async fn start_player_extraction(
        &self,
        pkey: &PlayerKey,
        ship_id: &ShipId,
    ) -> Result<ExtractionInfo, Errcode> {
        let (_, player) = self.get_player(pkey).await?;
        let mut player = player.write().await;
        let hist = player.history.clone();
        let ship = player.get_ship_mut(ship_id)?;
        let galaxy = self.galaxy.read().await;
        let Some(planet) = galaxy.get_planet(&ship.position) else {
            return Err(Errcode::CannotExtractWithoutPlanet);
        };
        let info = ship.start_extraction(&planet).await?;
        hist.add(PlayerHistoryEvent::StartExtraction {
            ship: *ship_id,
            planet: planet.id,
            power_attr: ship.power_attribution.clone(),
            secs_before_cargo_full: info.time_fill_cargo,
        })
        .await;
        Ok(info)
    }

    pub async fn player_market_buy(
        &self,
        pkey: &PlayerKey,
        station_id: &StationId,
        resource: &Resource,
        amnt: f64,
    ) -> Result<MarketTx, Errcode> {
        let (_, player) = self.get_player(pkey).await?;
        let mut player = player.write().await;
        let Some(station) = player.stations.get(station_id) else {
            return Err(Errcode::NoSuchStation(*station_id));
        };
        let (tx, price) = station
            .buy_resource(&self.market, &player.id, resource, amnt)
            .await?;
        player.money -= tx.removed_money.unwrap();
        player.score -= tx.removed_money.unwrap();
        player
            .history
            .add(PlayerHistoryEvent::BuyResource {
                station: *station_id,
                resource: *resource,
                price,
                fees: tx.fees,
                bought: tx.added_cargo.unwrap().1,
                spent: tx.removed_money.unwrap(),
            })
            .await;
        Ok(tx)
    }

    pub async fn player_market_sell(
        &self,
        pkey: &PlayerKey,
        station_id: &StationId,
        resource: &Resource,
        amnt: f64,
    ) -> Result<MarketTx, Errcode> {
        let (_, player) = self.get_player(pkey).await?;
        let mut player = player.write().await;
        let Some(station) = player.stations.get(station_id) else {
            return Err(Errcode::NoSuchStation(*station_id));
        };
        let (tx, price) = station
            .sell_resource(&self.market, &player.id, resource, amnt)
            .await?;
        player.money += tx.added_money.unwrap();
        player.score += tx.added_money.unwrap();
        player
            .history
            .add(PlayerHistoryEvent::SellResource {
                station: *station_id,
                resource: *resource,
                price,
                fees: tx.fees,
                sold: tx.removed_cargo.unwrap().1,
                earned: tx.added_money.unwrap(),
            })
            .await;
        Ok(tx)
    }

    pub async fn update_ships_owners(&self, pkey: &PlayerKey) -> Result<(), Errcode> {
        let (pid, player) = self.get_player(pkey).await?;
        let player = player.read().await;
        let mut ships = self.ship_owners.write().await;
        for shipid in player.ships.keys() {
            ships.insert(*shipid, pid);
        }
        Ok(())
    }

    pub async fn get_ship_owner(&self, ship_id: &ShipId) -> Option<PlayerId> {
        let ships = self.ship_owners.read().await;
        ships.get(ship_id).cloned()
    }

    pub async fn reset(&self) {
        self.taken_names.reset().await;
        self.player_index.reset().await;
        self.players.reset().await;
        let mut init_station = self.init_station.write().await;
        *self.ship_owners.write().await = BTreeMap::new();
        let mut new_galaxy = Galaxy::init();
        *init_station = new_galaxy.init_new_station().await;
        *self.galaxy.write().await = new_galaxy;
        self.market.reset().await;
        self.fifo_events.write().await.reset().await;
    }

    #[inline]
    pub async fn notify_all_players(&self, ev: SyslogEvent) {
        let all_players = self.players.get_all_keys().await;
        for pid in all_players {
            self.syslog.event(&pid, ev.clone()).await;
        }
    }
}
