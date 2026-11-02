use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::Hasher;
use std::sync::Arc;
use std::time::Instant;

use rand::Rng;

use crate::config::get_config;
use crate::crew::{CrewId, CrewMemberType};
use crate::errors::Errcode;
use crate::galaxy::station::{Station, StationId};
use crate::history::{AssignedCrew, PlayerHistory, PlayerHistoryEvent};
use crate::ship::cargo::ShipCargo;
use crate::ship::module::{ShipModuleId, ShipModuleType};
use crate::ship::upgrade::{ShipExpandUpgrade, ShipUpgrade};
use crate::ship::{Ship, ShipId};
use crate::syslog::{SyslogEvent, SyslogRecv};

pub type PlayerId = u64;
pub type PlayerKey = [u8; 128];

// Game state for a single player
pub struct Player {
    pub created: Instant,
    pub id: PlayerId,
    pub key: PlayerKey,
    pub score: f64,
    pub lost: bool,

    pub name: String,
    pub money: f64,
    pub costs: f64,

    pub stations: BTreeMap<StationId, Arc<Station>>,
    pub ships: BTreeMap<ShipId, Ship>,
    pub history: PlayerHistory,
}

impl Player {
    pub fn new(initstation: (StationId, Arc<Station>), name: String) -> Player {
        let mut hasher = DefaultHasher::new();
        hasher.write(name.as_bytes());
        let mut rng = rand::rng();
        let mut randbytes = [0; 128];
        rng.fill_bytes(&mut randbytes);

        let cfg = get_config();
        #[allow(unused_mut)]
        let mut money = cfg.game.player_init_money;

        #[cfg(feature = "testing")]
        if name.starts_with("test-rich") {
            money *= 10000.0;
        }
        let mut stations = BTreeMap::new();
        stations.insert(initstation.0, initstation.1);
        Player {
            created: Instant::now(),
            key: randbytes,
            id: hasher.finish(),
            lost: false,

            money,
            score: 0.0,
            costs: 0.0,

            name,
            stations,
            ships: BTreeMap::new(),
            history: PlayerHistory::empty(),
        }
    }

    pub fn get_ship<'a>(&'a self, id: &ShipId) -> Result<&'a Ship, Errcode> {
        self.ships.get(id).ok_or(Errcode::ShipNotFound(*id))
    }

    pub fn get_ship_mut<'a>(&'a mut self, id: &ShipId) -> Result<&'a mut Ship, Errcode> {
        self.ships.get_mut(id).ok_or(Errcode::ShipNotFound(*id))
    }

    #[inline]
    pub fn ship_in_station(&self, ship: &ShipId, station: &StationId) -> Result<bool, Errcode> {
        let ship = self.get_ship(ship)?;
        let Some(station) = self.stations.get(station) else {
            return Err(Errcode::NoSuchStation(*station));
        };
        Ok(ship.position == station.position)
    }

    //// Interfaces for game

    pub async fn update_costs(&mut self) {
        self.costs = 0.0;

        for station in self.stations.values() {
            // Deadlock because of this
            self.costs += station.sum_all_wages(&self.id).await;
        }
        self.costs += self
            .ships
            .values()
            .map(|ship| ship.crew.sum_wages())
            .sum::<f64>();
    }

    pub async fn update_money(&mut self, syslog: &SyslogRecv, tdelta: f64) {
        let before = self.money < (self.costs * 60.0);
        self.money -= self.costs * tdelta;
        let after = self.money < (self.costs * 60.0);
        if after && !before {
            let tleft = std::time::Duration::from_secs_f64(self.money / self.costs);
            syslog.event(self.id, SyslogEvent::LowFunds(tleft)).await;
        }
        if self.money < 0.0 && !self.lost {
            self.lost = true;
            syslog.event(self.id, SyslogEvent::GameLost).await;
            // TODO (#10)  Allow to create a new game with the same name if old one lost
            // TODO (#10)  What to do with its resources, ships, etc...
        }
    }

    pub async fn buy_ship(
        &mut self,
        station_id: &StationId,
        ship_id: &ShipId,
    ) -> Result<ShipId, Errcode> {
        let Some(station) = self.stations.get(station_id) else {
            return Err(Errcode::NoSuchStation(*station_id));
        };

        let ship_opt = {
            let mut data = None;
            let shipyard = station.shipyard.read().await;
            for (n, ship) in shipyard.iter().enumerate() {
                if &ship.id == ship_id {
                    data = Some((n, ship.compute_price()));
                }
            }
            data
        };

        let Some((index, price)) = ship_opt else {
            return Err(Errcode::ShipNotFound(*ship_id));
        };

        if price > self.money {
            return Err(Errcode::NotEnoughMoney(self.money, price));
        }

        let mut ship = station.buy_ship(index).await;
        let ship_id = ship.id;
        ship.owner = self.id;
        self.money -= price;
        self.ships.insert(ship_id, ship);
        self.history
            .add(PlayerHistoryEvent::BuyShip {
                ship: ship_id,
                station: station.id,
                price,
            })
            .await;
        Ok(ship_id)
    }

    pub async fn buy_ship_module(
        &mut self,
        station_id: &StationId,
        ship_id: &ShipId,
        modtype: ShipModuleType,
    ) -> Result<ShipModuleId, Errcode> {
        if !self.ship_in_station(ship_id, station_id)? {
            return Err(Errcode::ShipNotInStation);
        }
        let ship = self.ships.get_mut(ship_id).unwrap();

        let price = modtype.get_price_buy();
        if self.money < price {
            return Err(Errcode::NotEnoughMoney(self.money, price));
        }
        self.money -= price;
        let id = (ship.modules.len() + 1) as ShipModuleId;
        ship.modules.insert(id, modtype.new_module());

        let ship_id = ship.id;
        self.history
            .add(PlayerHistoryEvent::BuyShipModule {
                ship: ship_id,
                module: id,
                modtype,
                price,
                station: *station_id,
            })
            .await;
        Ok(id)
    }

    pub async fn buy_ship_upgrade(
        &mut self,
        station: &StationId,
        ship_id: &ShipId,
        upgrade: &ShipUpgrade,
    ) -> Result<f64, Errcode> {
        let ship = self
            .ships
            .get_mut(ship_id)
            .ok_or(Errcode::ShipNotFound(*ship_id))?;
        let Some(station) = self.stations.get(station).cloned() else {
            return Err(Errcode::NoSuchStation(*station));
        };

        let price = station.get_ship_upgrade_price(ship, upgrade);
        if price > self.money {
            return Err(Errcode::NotEnoughMoney(self.money, price));
        }

        self.money -= price;
        upgrade.install(ship);
        self.history
            .add(PlayerHistoryEvent::UpgradeShip {
                station: station.id,
                ship: *ship_id,
                upgrade: *upgrade,
                price,
            })
            .await;
        Ok(price)
    }

    pub async fn buy_ship_expansion(
        &mut self,
        station: &StationId,
        ship_id: &ShipId,
        expansion: &ShipExpandUpgrade,
    ) -> Result<f64, Errcode> {
        let ship = self
            .ships
            .get_mut(ship_id)
            .ok_or(Errcode::ShipNotFound(*ship_id))?;
        let Some(station) = self.stations.get(station).cloned() else {
            return Err(Errcode::NoSuchStation(*station));
        };

        let price = expansion.get_price() * station.get_inflation_ratio();
        if price > self.money {
            return Err(Errcode::NotEnoughMoney(self.money, price));
        }

        self.money -= price;
        expansion.install(ship);
        self.history
            .add(PlayerHistoryEvent::ExpandShip {
                station: station.id,
                ship: *ship_id,
                price,
                expansion: *expansion,
            })
            .await;
        Ok(price)
    }

    pub async fn buy_ship_module_upgrade(
        &mut self,
        station_id: &StationId,
        ship_id: &ShipId,
        mod_id: &ShipModuleId,
    ) -> Result<(f64, u8), Errcode> {
        if !self.ship_in_station(ship_id, station_id)? {
            return Err(Errcode::ShipNotInStation);
        }
        // SAFETY Checked on the function above
        let ship = self.ships.get_mut(ship_id).unwrap();
        let Some(ref mut module) = ship.modules.get_mut(mod_id) else {
            return Err(Errcode::NoSuchModule(*mod_id));
        };
        let price = module.price_next_rank();
        if price > self.money {
            return Err(Errcode::NotEnoughMoney(self.money, price));
        }

        self.money -= price;
        module.rank += 1;

        let newrank = module.rank;
        self.history
            .add(PlayerHistoryEvent::UpgradeModule {
                ship: *ship_id,
                station: *station_id,
                module: *mod_id,
                newrank,
                price,
            })
            .await;
        Ok((price, newrank))
    }

    pub async fn upgrade_ship_crew(
        &mut self,
        station_id: &StationId,
        ship_id: &ShipId,
        crew_id: &CrewId,
    ) -> Result<(f64, u8), Errcode> {
        if !self.ship_in_station(ship_id, station_id)? {
            return Err(Errcode::ShipNotInStation);
        };
        // SAFETY Checked in function above
        let ship = self.ships.get_mut(ship_id).unwrap();
        let assignation;
        let res = {
            let Some(ref mut cm) = ship.crew.0.get_mut(crew_id) else {
                return Err(Errcode::CrewMemberNotFound(*crew_id));
            };
            assignation = match cm.member_type {
                CrewMemberType::Pilot => AssignedCrew::Pilot { ship: *ship_id },
                CrewMemberType::Operator => {
                    let matchmod = ship
                        .modules
                        .iter()
                        .filter(|(_, m)| m.operator.map(|v| &v == crew_id).unwrap_or(false))
                        .nth(0)
                        .unwrap();
                    AssignedCrew::ShipModule {
                        ship: *ship_id,
                        module: *matchmod.0,
                    }
                }
                CrewMemberType::Trader | CrewMemberType::Soldier => unreachable!(),
            };

            let price = cm.price_next_rank();
            if price > self.money {
                return Err(Errcode::NotEnoughMoney(self.money, price));
            }

            self.money -= price;
            cm.rank += 1;
            (price, cm.rank)
        };
        ship.update_perf_stats();
        self.history
            .add(PlayerHistoryEvent::UpgradeCrew {
                assignation,
                crew: *crew_id,
                price: res.0,
                newrank: res.1,
                station: *station_id,
            })
            .await;
        Ok(res)
    }

    pub async fn upgrade_station_crew(
        &mut self,
        station_id: &StationId,
        crew_id: &CrewId,
    ) -> Result<(f64, u8), Errcode> {
        let Some(station) = self.stations.get(station_id) else {
            return Err(Errcode::NoSuchStation(*station_id));
        };
        let res = station
            .upgrade_station_crew(&self.id, &mut self.money, crew_id)
            .await;
        match res {
            Ok((price, newrank)) => {
                self.update_costs().await;
                self.history
                    .add(PlayerHistoryEvent::UpgradeCrew {
                        assignation: AssignedCrew::Trader {
                            station: *station_id,
                        },
                        crew: *crew_id,
                        price,
                        newrank,
                        station: *station_id,
                    })
                    .await;
                Ok((price, newrank))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn buy_station_cargo(
        &mut self,
        station_id: &StationId,
        amnt: usize,
    ) -> Result<ShipCargo, Errcode> {
        let Some(station) = self.stations.get_mut(station_id) else {
            return Err(Errcode::NoSuchStation(*station_id));
        };
        let cost = (amnt as f64) * station.cargo_price(&self.id).await;
        if cost > self.money {
            return Err(Errcode::NotEnoughMoney(self.money, cost));
        }
        self.money -= cost;
        let newcargo = station.add_cargo_cap(&self.id, amnt).await;
        self.history
            .add(PlayerHistoryEvent::BuyStationCargo {
                station: *station_id,
                amnt,
                total_price: cost,
            })
            .await;
        Ok(newcargo)
    }

    pub async fn unassign_crew(
        &mut self,
        station_id: StationId,
        crew_id: CrewId,
    ) -> Result<(), Errcode> {
        let Some(station) = self.stations.get(&station_id) else {
            return Err(Errcode::NoSuchStation(station_id));
        };
        if station.unassign_crew(&self.id, &crew_id).await? {
            return Ok(());
        }
        let pd = station.player_data.clone_val(&self.id).await.unwrap();
        let mut pd = pd.write().await;

        for ship in self.ships.values_mut() {
            if ship.crew.0.contains_key(&crew_id) {
                if ship.position != station.position {
                    return Err(Errcode::ShipNotInStation);
                }

                let c = ship.crew.0.remove(&crew_id).unwrap();
                let old = match c.member_type {
                    crate::crew::CrewMemberType::Pilot => {
                        ship.pilot = None;
                        AssignedCrew::Pilot { ship: ship.id }
                    }
                    crate::crew::CrewMemberType::Operator => {
                        let mut mod_id = None;
                        for (mid, module) in ship.modules.iter_mut() {
                            if let Some(id) = module.operator {
                                if id == crew_id {
                                    module.operator = None;
                                    mod_id = Some(*mid);
                                    break;
                                }
                            }
                        }
                        let mod_id = mod_id.unwrap();
                        AssignedCrew::ShipModule {
                            ship: ship.id,
                            module: mod_id,
                        }
                    }
                    _ => unreachable!(),
                };
                pd.idle_crew.0.insert(crew_id, c);
                self.history
                    .add(PlayerHistoryEvent::UnassignCrew {
                        old_assignation: old,
                        crew: crew_id,
                        station: station_id,
                    })
                    .await;
                return Ok(());
            }
        }
        Err(Errcode::CrewMemberNotFound(crew_id))
    }

    pub fn export_json(&self, full: bool) -> serde_json::Value {
        if !full {
            return serde_json::json!({
                "id": self.id,
                "name": self.name,
            });
        }

        let stations = self.stations.keys().cloned().collect::<Vec<StationId>>();
        let ships =
            serde_json::to_value(self.ships.keys().cloned().collect::<Vec<ShipId>>()).unwrap();
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "stations": stations,
            "money": self.money,
            "ships": ships,
            "costs": self.costs,
        })
    }

    pub fn export_ship_json(&self, id: &ShipId) -> Option<serde_json::Value> {
        let ship = self.ships.get(id)?;
        Some(ship.export_json(true))
    }
}
