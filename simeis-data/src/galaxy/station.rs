use std::collections::BTreeMap;
use std::sync::Arc;

use mea::rwlock::RwLock;
use rand::RngExt;
use serde::{Deserialize, Serialize};

use crate::config::get_config;
use crate::crew::{Crew, CrewId, CrewMember, CrewMemberType};
use crate::errors::Errcode;
use crate::industry::{IndustryUnit, IndustryUnitId, IndustryUnitType};
use crate::market::{fee_rate, Market, MarketTx};
use crate::player::{Player, PlayerId};
use crate::ship::cargo::ShipCargo;
use crate::ship::module::ShipModuleId;
use crate::ship::resources::Resource;
use crate::ship::upgrade::ShipUpgrade;
use crate::ship::Ship;
use crate::syslog::{SyslogEvent, SyslogRecv};
use crate::utils::ShardedLockedData;

use super::scan::ScanResult;
use super::{Galaxy, SpaceCoord};

pub type StationId = u16;

#[derive(Serialize, Deserialize, Debug)]
pub struct StationInfo {
    pub id: StationId,
    pub position: SpaceCoord,
}

impl StationInfo {
    // TODO (#8) Based on the scanner rank, get informations on crew and cargo
    pub fn scan(_rank: u8, station: &Station) -> StationInfo {
        StationInfo {
            id: station.id,
            position: station.position,
        }
    }
}

#[derive(Default, Debug, Serialize)]
pub struct StationPlayerData {
    pub idle_crew: Crew,
    pub crew: Crew,
    pub trader: Option<CrewId>,
    pub cargo: ShipCargo,
    pub industry: BTreeMap<IndustryUnitId, IndustryUnit>,
}

impl StationPlayerData {
    pub fn new() -> StationPlayerData {
        let cfg = get_config();
        StationPlayerData {
            cargo: ShipCargo::with_capacity(cfg.station.init_cargo),
            ..Default::default()
        }
    }
}

pub struct Station {
    pub id: StationId,
    pub position: SpaceCoord,
    pub shipyard: RwLock<Vec<Ship>>,

    pub player_data: ShardedLockedData<PlayerId, Arc<RwLock<StationPlayerData>>>,
}

impl std::fmt::Debug for Station {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Station")
            .field("id", &self.id)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl Station {
    pub fn init(id: u16, position: super::SpaceCoord) -> Station {
        let cfg = get_config();
        Station {
            id,
            position,
            shipyard: RwLock::new(Ship::init_shipyard(position)),
            player_data: ShardedLockedData::new(cfg.game.shards),
        }
    }

    // TODO (#8) Allow to build improvements for the scanner
    pub fn scan(&self, galaxy: &Galaxy) -> ScanResult {
        galaxy.scan_sector(u8::MAX, &self.position)
    }

    pub async fn cargo_price(&self, player: &PlayerId) -> f64 {
        let cfg = get_config();
        let cap = if let Some(data) = self.player_data.clone_val(player).await {
            data.read().await.cargo.capacity
        } else {
            cfg.station.init_cargo
        };
        let capdiff = cap - cfg.station.init_cargo;
        if capdiff < cfg.station.cargo_price_incdiv {
            cfg.station.cargo_base_price
        } else {
            let mult = 1.0 + (capdiff / cfg.station.cargo_price_incdiv).ln();
            cfg.station.cargo_base_price * mult
        }
    }

    pub async fn buy_industry(
        &self,
        player: &mut Player,
        unit: IndustryUnitType,
    ) -> Result<(IndustryUnitId, f64), Errcode> {
        let cost = unit.get_price_buy();
        if player.money < cost {
            return Err(Errcode::NotEnoughMoney(player.money, cost));
        }
        self.ensure_has_player_data(&player.id).await;
        let pd = self.player_data.clone_val(&player.id).await.unwrap();
        let mut pd = pd.write().await;
        let unit = unit.new_unit();
        let unit_id = unit.id;
        pd.industry.insert(unit_id, unit);
        player.money -= cost;
        Ok((unit_id, cost))
    }

    pub async fn upgrade_industry(
        &self,
        player: &mut Player,
        id: &IndustryUnitId,
    ) -> Result<u8, Errcode> {
        self.ensure_has_player_data(&player.id).await;
        let pd = self.player_data.clone_val(&player.id).await.unwrap();
        let mut pd = pd.write().await;
        let Some(unit) = pd.industry.get_mut(id) else {
            return Err(Errcode::NoSuchIndustryUnit);
        };
        let cost = unit.price_next_rank();
        if cost > player.money {
            return Err(Errcode::NotEnoughMoney(player.money, cost));
        }
        player.money -= cost;
        unit.rank += 1;
        Ok(unit.rank)
    }

    pub async fn start_industry(
        &self,
        player: &PlayerId,
        id: &IndustryUnitId,
    ) -> Result<(), Errcode> {
        self.ensure_has_player_data(player).await;
        let pd = self.player_data.clone_val(player).await.unwrap();
        let mut pd = pd.write().await;
        let Some(unit) = pd.industry.get_mut(id) else {
            return Err(Errcode::NoSuchIndustryUnit);
        };
        unit.started = true;
        Ok(())
    }

    pub async fn stop_industry(
        &self,
        player: &PlayerId,
        id: &IndustryUnitId,
    ) -> Result<(), Errcode> {
        self.ensure_has_player_data(player).await;
        let pd = self.player_data.clone_val(player).await.unwrap();
        let mut pd = pd.write().await;
        let Some(unit) = pd.industry.get_mut(id) else {
            return Err(Errcode::NoSuchIndustryUnit);
        };
        unit.started = false;
        Ok(())
    }

    pub async fn buy_cargo(&self, player: &mut Player, amnt: &usize) -> Result<ShipCargo, Errcode> {
        let cost = (*amnt as f64) * self.cargo_price(&player.id).await;
        if cost > player.money {
            return Err(Errcode::NotEnoughMoney(player.money, cost));
        }
        player.money -= cost;
        self.ensure_has_player_data(&player.id).await;
        let pd = self.player_data.clone_val(&player.id).await.unwrap();
        let mut pd = pd.write().await;
        pd.cargo.capacity += *amnt as f64;
        Ok(pd.cargo.clone())
    }

    pub async fn add_cargo_cap(&self, player: &PlayerId, amnt: usize) -> ShipCargo {
        self.ensure_has_player_data(player).await;
        let pd = self.player_data.clone_val(player).await.unwrap();
        let mut pd = pd.write().await;
        pd.cargo.capacity += amnt as f64;
        pd.cargo.clone()
    }

    pub async fn assign_trader(&self, pid: &PlayerId, id: CrewId) -> Result<(), Errcode> {
        self.ensure_has_player_data(pid).await;
        let pd = self.player_data.clone_val(pid).await.unwrap();
        let mut pd = pd.write().await;
        let Some(cm) = pd.idle_crew.0.remove(&id) else {
            if pd.crew.0.contains_key(&id) {
                return Err(Errcode::CrewMemberNotIdle(id));
            } else {
                return Err(Errcode::CrewMemberNotFound(id));
            }
        };

        pd.crew.0.insert(id, cm);
        pd.trader = Some(id);
        Ok(())
    }

    pub async fn onboard_pilot(&self, ship: &mut Ship, id: &CrewId) -> Result<(), Errcode> {
        self.ensure_has_player_data(&ship.owner).await;
        let pd = self.player_data.clone_val(&ship.owner).await.unwrap();
        let mut pd = pd.write().await;
        let Some(cm) = pd.idle_crew.0.get(id) else {
            return Err(Errcode::CrewMemberNotIdle(*id));
        };
        if cm.member_type != CrewMemberType::Pilot {
            return Err(Errcode::WrongCrewType(CrewMemberType::Pilot));
        }
        ship.pilot = Some(*id);
        let pilot = pd.idle_crew.0.remove(id).unwrap();
        ship.crew.0.insert(*id, pilot);
        ship.update_perf_stats();
        Ok(())
    }

    pub async fn onboard_operator(
        &self,
        ship: &mut Ship,
        id: &CrewId,
        mod_id: &ShipModuleId,
    ) -> Result<(), Errcode> {
        self.ensure_has_player_data(&ship.owner).await;
        let cm = self
            .get_idle_crew(&ship.owner, id, CrewMemberType::Operator)
            .await?;
        let Some(module) = ship.modules.get_mut(mod_id) else {
            return Err(Errcode::NoSuchModule(*mod_id));
        };
        if !module.need(&cm.member_type) {
            return Err(Errcode::CrewNotNeeded);
        }
        module.operator = Some(*id);
        let pd = self.player_data.clone_val(&ship.owner).await.unwrap();
        let mut pd = pd.write().await;
        ship.crew.0.insert(*id, pd.idle_crew.0.remove(id).unwrap());
        Ok(())
    }

    pub async fn unassign_crew(&self, pid: &PlayerId, crew_id: &CrewId) -> Result<bool, Errcode> {
        let pd = self.player_data.clone_val(pid).await.unwrap();
        let mut pd = pd.write().await;
        if pd.idle_crew.0.contains_key(crew_id) {
            return Err(Errcode::CrewMemberIdle(*crew_id));
        }
        if let Some(c) = pd.crew.0.remove(crew_id) {
            match c.member_type {
                CrewMemberType::Operator => {
                    let mut found = false;
                    for unit in pd.industry.values_mut() {
                        if let Some(ref id) = unit.operator {
                            if id == crew_id {
                                unit.operator = None;
                                found = true;
                                break;
                            }
                        }
                    }
                    assert!(found);
                }
                CrewMemberType::Trader => pd.trader = None,
                _ => unreachable!(),
            }
            pd.idle_crew.0.insert(*crew_id, c);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn assign_crew_to_industry(
        &self,
        pid: &PlayerId,
        id: &CrewId,
        iid: &IndustryUnitId,
    ) -> Result<(), Errcode> {
        let cm = self
            .get_idle_crew(pid, id, CrewMemberType::Operator)
            .await?;
        let pd = self.player_data.clone_val(pid).await.unwrap();
        let mut pd = pd.write().await;
        let Some(industry) = pd.industry.get_mut(iid) else {
            return Err(Errcode::NoSuchIndustryUnit);
        };
        if !industry.need_crew_member(&cm.member_type) {
            return Err(Errcode::CrewNotNeeded);
        }
        industry.assign_operator(*id, &cm);
        let cm = pd.idle_crew.0.remove(id).unwrap();
        pd.crew.0.insert(*id, cm);
        Ok(())
    }

    pub async fn get_idle_crew(
        &self,
        pid: &PlayerId,
        id: &CrewId,
        ctype: CrewMemberType,
    ) -> Result<CrewMember, Errcode> {
        self.ensure_has_player_data(pid).await;
        let pd = self.player_data.clone_val(pid).await.unwrap();
        let pd = pd.read().await;
        let Some(cm) = pd.idle_crew.0.get(id) else {
            return Err(Errcode::CrewMemberNotIdle(*id));
        };
        if cm.member_type != ctype {
            return Err(Errcode::WrongCrewType(ctype));
        }
        Ok(cm.clone())
    }

    pub async fn buy_resource(
        &self,
        market: &Market,
        player: &PlayerId,
        resource: &Resource,
        amnt: f64,
    ) -> Result<(MarketTx, f64), Errcode> {
        self.ensure_has_player_data(player).await;
        let pd = self.player_data.clone_val(player).await.unwrap();
        let mut pd = pd.write().await;
        let Some(trader) = pd.trader else {
            return Err(Errcode::NoTraderAssigned);
        };
        let cm = pd.crew.0.get(&trader).unwrap();
        let can_cargo = pd.cargo.space_for(resource);
        let amnt = amnt.min(can_cargo);
        if amnt == 0.0 {
            return Err(Errcode::BuyNothing);
        }
        let (tx, price) = market.buy(cm, resource, amnt).await;
        let (r, a) = tx.added_cargo.unwrap();
        pd.cargo.add_resource(&r, a);
        Ok((tx, price))
    }

    pub async fn sell_resource(
        &self,
        market: &Market,
        player: &PlayerId,
        resource: &Resource,
        amnt: f64,
    ) -> Result<(MarketTx, f64), Errcode> {
        self.ensure_has_player_data(player).await;
        let pd = self.player_data.clone_val(player).await.unwrap();
        let mut pd = pd.write().await;
        let Some(trader) = pd.trader else {
            return Err(Errcode::NoTraderAssigned);
        };
        let cm = pd.crew.0.get(&trader).unwrap();
        let Some(can_cargo) = pd.cargo.resources.get(resource) else {
            return Err(Errcode::SellNothing);
        };
        let amnt = amnt.min(*can_cargo);
        if amnt <= 0.0 {
            return Err(Errcode::SellNothing);
        }
        let (tx, price) = market.sell(cm, resource, amnt).await;
        let (r, a) = tx.removed_cargo.unwrap();
        let unloaded = pd.cargo.unload(&r, a);
        debug_assert_eq!(unloaded, a);
        Ok((tx, price))
    }

    pub async fn refuel_ship(&self, ship: &mut Ship) -> Result<f64, Errcode> {
        if self.position != ship.position {
            return Err(Errcode::ShipNotInStation);
        }
        let Some(pd) = self.player_data.clone_val(&ship.owner).await else {
            return Err(Errcode::NoFuelInCargo);
        };
        let mut pd = pd.write().await;
        let Some(qty) = pd.cargo.resources.get(&Resource::Fuel) else {
            return Err(Errcode::NoFuelInCargo);
        };
        if *qty == 0.0 {
            return Err(Errcode::NoFuelInCargo);
        }
        debug_assert!(ship.fuel_tank >= 0.0);
        debug_assert!(ship.fuel_tank_capacity >= ship.fuel_tank);
        let needed = ship.fuel_tank_capacity - ship.fuel_tank;
        let unload = needed.min(*qty);
        let unloaded = pd.cargo.unload(&Resource::Fuel, unload);
        ship.fuel_tank += unloaded;
        debug_assert!(ship.fuel_tank_capacity >= ship.fuel_tank);
        Ok(unloaded)
    }

    pub async fn repair_ship(&self, ship: &mut Ship) -> Result<f64, Errcode> {
        if self.position != ship.position {
            return Err(Errcode::ShipNotInStation);
        }
        let Some(pd) = self.player_data.clone_val(&ship.owner).await else {
            return Err(Errcode::NoHullInCargo);
        };
        let mut pd = pd.write().await;
        let Some(qty) = pd.cargo.resources.get(&Resource::Hull) else {
            return Err(Errcode::NoHullInCargo);
        };
        if *qty == 0.0 {
            return Err(Errcode::NoHullInCargo);
        }
        debug_assert!(ship.hull_resistance >= ship.hull_decay);

        let amnt = ship.hull_decay.min(*qty);
        if amnt == 0.0 {
            return Ok(0.0);
        }
        let unloaded = pd.cargo.unload(&Resource::Hull, amnt);
        ship.hull_decay -= unloaded;
        debug_assert!(
            ship.hull_resistance >= ship.hull_decay,
            "{} < {}",
            ship.hull_resistance,
            ship.hull_decay
        );
        debug_assert!(ship.hull_decay >= 0.0, "{}", ship.hull_decay);
        debug_assert!(unloaded >= 0.0, "{}", unloaded);
        Ok(unloaded)
    }

    pub fn get_inflation_ratio(&self) -> f64 {
        // TODO (#9) Modify inflation based on station economy metrics
        1.0
    }

    pub fn get_ship_upgrade_price(&self, _ship: &Ship, upgrade: &ShipUpgrade) -> f64 {
        // TODO (#9) Modify price based on upgrades already installed on the ship
        upgrade.get_price() * self.get_inflation_ratio()
    }

    pub async fn get_cargo_potential_price(&self, id: &PlayerId) -> f64 {
        let Some(pd) = self.player_data.clone_val(id).await else {
            return 0.0;
        };
        let pd = pd.read().await;
        pd.cargo
            .resources
            .iter()
            .map(|(r, amnt)| r.base_price() * amnt)
            .sum()
    }

    pub async fn add_resource(&self, id: &PlayerId, resource: &Resource, amnt: f64) -> f64 {
        self.ensure_has_player_data(id).await;
        let pd = self.player_data.clone_val(id).await.unwrap();
        let mut pd = pd.write().await;
        pd.cargo.add_resource(resource, amnt)
    }

    // TODO (#45)    Every X secs in game, update the shipyard
    //    If len of shipyard is < 10, add a random one on top of existing ones
    //    If len == 10, take one of the last 7 ships, and replace it with random

    pub async fn buy_ship(&self, index: usize) -> Ship {
        let mut shipyard = self.shipyard.write().await;

        // Ship starters, always keep them
        let mut ship = if index < 3 {
            let ship = shipyard.get_mut(index).unwrap();
            let got = ship.clone();
            ship.id = rand::rng().random();
            got
        } else {
            let ship = shipyard.remove(index);
            shipyard.push(Ship::random(self.position));
            ship
        };
        ship.update_perf_stats();
        ship.fuel_tank = ship.fuel_tank_capacity;
        ship
    }

    pub async fn ensure_has_player_data(&self, id: &PlayerId) {
        if !self.player_data.contains_key(id).await {
            let pd = Arc::new(RwLock::new(StationPlayerData::new()));
            self.player_data.insert(*id, pd).await;
        }
    }

    pub async fn sum_all_wages(&self, id: &PlayerId) -> f64 {
        let Some(pd) = self.player_data.clone_val(id).await else {
            return 0.0;
        };
        let pd = pd.read().await;
        pd.crew.sum_wages() + pd.idle_crew.sum_wages()
    }

    pub async fn upgrade_station_crew(
        &self,
        id: &PlayerId,
        money: &mut f64,
        crew: &CrewId,
    ) -> Result<(f64, u8), Errcode> {
        let Some(pd) = self.player_data.clone_val(id).await else {
            return Err(Errcode::CrewMemberNotFound(*crew));
        };
        let mut pd = pd.write().await;

        let Some(cm) = pd.crew.0.get_mut(crew) else {
            return Err(Errcode::CrewMemberNotFound(*crew));
        };
        let price = cm.price_next_rank();
        if price > *money {
            return Err(Errcode::NotEnoughMoney(*money, price));
        }
        *money -= price;
        cm.rank += 1;
        Ok((price, cm.rank))
    }

    pub async fn hire_crew(&self, id: &PlayerId, crewtype: CrewMemberType) -> CrewId {
        let crewid = rand::rng().random();
        let member = CrewMember::from(crewtype);

        self.ensure_has_player_data(id).await;
        let pd = self.player_data.clone_val(id).await.unwrap();
        let mut pd = pd.write().await;
        pd.idle_crew.0.insert(crewid, member);
        crewid
    }

    pub async fn fire_crew(&self, id: &PlayerId, crewid: &CrewId) -> Result<CrewMember, Errcode> {
        self.ensure_has_player_data(id).await;
        let pd = self.player_data.clone_val(id).await.unwrap();
        let mut pd = pd.write().await;
        let Some(cm) = pd.idle_crew.0.remove(crewid) else {
            return Err(Errcode::CrewMemberNotFound(*crewid));
        };
        Ok(cm)
    }

    pub async fn upgr_trader_price(&self, id: &PlayerId) -> Option<f64> {
        let pd = self.player_data.clone_val(id).await?;
        let pd = pd.read().await;
        pd.trader.map(|trader| {
            let cm = pd.crew.0.get(&trader).unwrap();
            cm.price_next_rank()
        })
    }

    pub async fn clone_cargo(&self, id: &PlayerId) -> ShipCargo {
        let Some(pd) = self.player_data.clone_val(id).await else {
            let cfg = get_config();
            return ShipCargo::with_capacity(cfg.station.init_cargo);
        };
        let pd = pd.read().await;
        pd.cargo.clone()
    }

    pub async fn get_fee_rate(&self, id: &PlayerId) -> Result<f64, Errcode> {
        let Some(pd) = self.player_data.clone_val(id).await else {
            return Err(Errcode::NoTraderAssigned);
        };
        let pd = pd.read().await;
        let Some(trader) = pd.trader else {
            return Err(Errcode::NoTraderAssigned);
        };
        let cm = pd.crew.0.get(&trader).unwrap();
        Ok(fee_rate(cm.rank))
    }

    pub async fn to_json(&self, id: &PlayerId) -> serde_json::Value {
        if let Some(pd) = self.player_data.clone_val(id).await {
            let pd = pd.read().await;
            self._to_json(&pd)
        } else {
            let pd = StationPlayerData::new();
            self._to_json(&pd)
        }
    }

    fn _to_json(&self, data: &StationPlayerData) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "position": self.position,
            "crew": data.crew,
            "cargo": data.cargo,
            "idle_crew": data.idle_crew,
            "trader": data.trader,
            "industry": data.industry,
        })
    }

    pub async fn to_json_all_players(&self) -> serde_json::Value {
        let mut players_data = BTreeMap::new();
        let all_players = self.player_data.get_all_keys().await;
        for pid in all_players {
            let pd = self.player_data.clone_val(&pid).await.unwrap();
            let pd = pd.read().await;
            players_data.insert(
                pid,
                serde_json::json!({
                    "crew": pd.crew,
                    "cargo": pd.cargo,
                    "idle_crew": pd.idle_crew,
                    "trader": pd.trader,
                    "industry": pd.industry,
                }),
            );
        }
        serde_json::json!({
            "id": self.id,
            "position": self.position,
            "players_data": players_data,
        })
    }

    pub async fn update_crafting(&self, syslog: &SyslogRecv, tdelta: f64, id: &PlayerId) {
        let Some(pd) = self.player_data.clone_val(id).await else {
            return;
        };
        let mut pd = pd.write().await;
        let StationPlayerData {
            cargo, industry, ..
        } = &mut *pd;
        for (iid, industry) in industry.iter_mut() {
            if let Some(ratio) = industry.can_work(&tdelta, &cargo.resources) {
                if ratio < 0.01 {
                    industry.started = false;
                    syslog.event(*id, SyslogEvent::IndustryStopped(*iid)).await;
                    continue;
                }
                let t = tdelta * ratio;
                industry.work(t, &mut cargo.resources);
            }
        }
    }

    pub async fn get_industry_production(
        &self,
        pid: &PlayerId,
        id: IndustryUnitId,
    ) -> Result<(Vec<(Resource, f64)>, Vec<(Resource, f64)>), Errcode> {
        let Some(pd) = self.player_data.clone_val(pid).await else {
            return Err(Errcode::NoSuchIndustryUnit);
        };

        let pd = pd.read().await;
        let Some(industry) = pd.industry.get(&id) else {
            return Err(Errcode::NoSuchIndustryUnit);
        };

        let Some(opid) = industry.operator else {
            return Ok((vec![], vec![]));
        };
        let op = pd.crew.0.get(&opid).unwrap();

        let inputs = industry.input(op.rank);
        let outputs = industry.output(op.rank);
        Ok((inputs, outputs))
    }

    pub async fn reset(&self) {
        self.player_data.reset().await;
        *self.shipyard.write().await = Ship::init_shipyard(self.position);
    }
}
