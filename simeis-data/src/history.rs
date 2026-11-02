use std::collections::BTreeMap;
use std::sync::Arc;
use std::{collections::VecDeque, time::Instant};

use mea::rwlock::RwLock;
use serde::Serialize;
use serde_json::Value;

use crate::crew::{CrewId, CrewMemberType};
use crate::galaxy::planet::PlanetId;
use crate::galaxy::station::StationId;
use crate::galaxy::SpaceCoord;
use crate::industry::{IndustryUnitId, IndustryUnitType};

use crate::ship::module::{ShipModuleId, ShipModuleType};
use crate::ship::resources::Resource;
use crate::ship::upgrade::{ShipExpandUpgrade, ShipUpgrade};
use crate::ship::ShipId;

pub const PLAYER_ACTION_HISTORY_LEN: usize = 100;

#[derive(strum::IntoStaticStr, Debug, Clone, Copy, Serialize)]
#[strum(serialize_all = "snake_case")]
pub enum AssignedCrew {
    Trader {
        station: StationId,
    },
    Pilot {
        ship: ShipId,
    },
    ShipModule {
        ship: ShipId,
        module: ShipModuleId,
    },
    IndustryUnit {
        station: StationId,
        unit: IndustryUnitId,
    },
}

#[derive(strum::IntoStaticStr, Debug, Clone, Serialize)]
#[strum(serialize_all = "snake_case")]
pub enum PlayerHistoryEvent {
    // Ship
    BuyShip {
        ship: ShipId,
        station: StationId,
        price: f64,
    },
    BuyShipModule {
        ship: ShipId,
        module: ShipModuleId,
        modtype: ShipModuleType,
        price: f64,
        station: StationId,
    },
    UpgradeModule {
        ship: ShipId,
        station: StationId,
        module: ShipModuleId,
        newrank: u8,
        price: f64,
    },
    UpgradeShip {
        ship: ShipId,
        upgrade: ShipUpgrade,
        price: f64,
        station: StationId,
    },
    ExpandShip {
        ship: ShipId,
        expansion: ShipExpandUpgrade,
        price: f64,
        station: StationId,
    },
    ShipNavigate {
        ship: ShipId,
        src: SpaceCoord,
        dest: SpaceCoord,
        fuel_cost: f64,
        hull_usage: f64,
        duration_secs: f64,
    },
    StopNavigation {
        ship: ShipId,
        position: SpaceCoord,
    },
    UnloadShip {
        ship: ShipId,
        station: StationId,
        emptied: bool,
        resources: Vec<Resource>,
    },
    StartExtraction {
        ship: ShipId,
        planet: PlanetId,
        power_attr: BTreeMap<Resource, u8>,
        secs_before_cargo_full: f64,
    },
    StopExtraction {
        ship: ShipId,
        cargo_usage_perc: f64,
    },

    // Station
    BuyStationCargo {
        station: StationId,
        amnt: usize,
        total_price: f64,
    },
    RefuelShip {
        station: StationId,
        ship: ShipId,
        amnt: f64,
        tank_full: bool,
    },
    RepairedShip {
        station: StationId,
        ship: ShipId,
        amnt: f64,
        fully_repaired: bool,
    },

    // Crew
    HireCrew {
        crewtype: CrewMemberType,
        crew: CrewId,
        station: StationId,
    },
    FireCrew {
        crew: CrewId,
        station: StationId,
    },
    AssignCrew {
        crew: CrewId,
        assignation: AssignedCrew,
    },
    UnassignCrew {
        crew: CrewId,
        station: StationId,
        old_assignation: AssignedCrew,
    },
    UpgradeCrew {
        assignation: AssignedCrew,
        crew: CrewId,
        price: f64,
        newrank: u8,
        station: StationId,
    },

    // Industry
    BuyIndustryUnit {
        station: StationId,
        unittype: IndustryUnitType,
        unit: IndustryUnitId,
    },
    StartIndustry {
        unit: IndustryUnitId,
    },
    StopIndustry {
        unit: IndustryUnitId,
    },

    // Market
    BuyResource {
        station: StationId,
        resource: Resource,
        price: f64,
        bought: f64,
        spent: f64,
        fees: f64,
    },
    SellResource {
        station: StationId,
        resource: Resource,
        price: f64,
        sold: f64,
        earned: f64,
        fees: f64,
    },
}

#[derive(Clone)]
pub struct PlayerHistory {
    data: Arc<RwLock<VecDeque<(Instant, PlayerHistoryEvent)>>>,
}

impl PlayerHistory {
    pub fn empty() -> PlayerHistory {
        PlayerHistory {
            data: Arc::new(RwLock::new(VecDeque::with_capacity(
                PLAYER_ACTION_HISTORY_LEN,
            ))),
        }
    }

    #[inline]
    pub async fn export(&self) -> Vec<Value> {
        let h = self.data.read().await;
        let mut res = Vec::with_capacity(h.len());
        for (t, el) in h.iter().rev() {
            let Value::Object(mut obj) = serde_json::to_value(el).unwrap() else {
                unreachable!();
            };
            debug_assert_eq!(obj.len(), 1);
            let mapkey = obj.keys().next().unwrap().clone();
            let Value::Object(mut data) = obj.remove(&mapkey).unwrap() else {
                unreachable!();
            };
            let key: &'static str = el.into();
            data.insert("action".to_string(), Value::String(key.to_string()));
            data.insert(
                "elapsed".to_string(),
                Value::Number(serde_json::Number::from_f64(t.elapsed().as_secs_f64()).unwrap()),
            );
            res.push(Value::Object(data));
        }
        res
    }

    pub async fn add(&self, event: PlayerHistoryEvent) {
        let mut history = self.data.write().await;
        debug_assert!(history.len() <= PLAYER_ACTION_HISTORY_LEN);
        history.push_back((Instant::now(), event));
        while history.len() > PLAYER_ACTION_HISTORY_LEN {
            history.pop_front();
        }
    }
}
