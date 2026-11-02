use std::collections::BTreeMap;

use ntex::router::IntoPattern;
use ntex::web::types::{Json, Path};
use ntex::web::{self, scope, HttpRequest, ServiceConfig};
use simeis_data::crew::{CrewMember, CrewMemberType};
use simeis_data::errors::Errcode;
use simeis_data::market::fee_rate;
use simeis_data::ship::cargo::calc_slowing_ratio;
use simeis_data::ship::module::{calc_extraction_rate, ShipModule, ShipModuleType};
use simeis_data::ship::resources::{apply_power_attribution, Resource};
use simeis_data::ship::upgrade::{ShipExpandUpgrade, ShipUpgrade};
use simeis_data::ship::ShipId;
use strum::IntoEnumIterator;

use crate::api::build_response;
use crate::GameState;
use serde::{Deserialize, Serialize};
use serde_json::{json, to_value};
use simeis_data::galaxy::{SpaceCoord, SpaceObject};
use simeis_data::ship::navigation::Travel;
use simeis_data::ship::shipstats::ShipStats;

#[derive(Serialize, Deserialize)]
pub struct NavigationComputation {
    ship_stats: ShipStats,
    origin: SpaceCoord,
    dest: SpaceCoord,
    cargo_usage_perc: u8,
}

// @summary Compute how much will cost a travel to a specific position (X, Y, Z)
// @returns Travel informations on this destination
#[web::post("/navigation")]
async fn compute_travel_costs(body: Json<NavigationComputation>) -> impl web::Responder {
    let NavigationComputation {
        ship_stats,
        origin,
        dest,
        cargo_usage_perc,
    } = body.0;
    if cargo_usage_perc > 100 {
        return build_response(Err(Errcode::InvalidArgument("cargo_usage_perc > 100")));
    }
    let slowing_ratio = calc_slowing_ratio((cargo_usage_perc as f64) / (u8::MAX as f64));
    let travel = Travel::new(dest);
    let cost = travel.compute_from_stats(&origin, &ship_stats, slowing_ratio);
    build_response(cost.map(|v| json!({"costs": v})))
}

#[derive(Serialize, Deserialize)]
pub struct ShipMiningRateComputation {
    planet_coord: SpaceCoord,
    #[serde(default)]
    power_attribution: BTreeMap<Resource, u8>,
}

// @summary Compute how much resources a given ship can be extract from a given planet
// @returns The mining rate for each resources
#[web::post("/mining_rate/{ship_id}")]
async fn compute_ship_mining_rate(
    srv: GameState,
    req: HttpRequest,
    ship_id: Path<ShipId>,
    body: Json<ShipMiningRateComputation>,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let ShipMiningRateComputation {
        planet_coord,
        power_attribution,
    } = body.0;

    let ship_data = srv
        .map_ship(&pkey, &ship_id, |_, _, ship| {
            Box::pin(async move {
                let mut result = vec![];
                for m in ship.modules.values() {
                    let Some(ref opid) = m.operator else {
                        continue;
                    };
                    let Some(cm) = ship.crew.0.get(opid) else {
                        continue;
                    };
                    result.push((m.modtype, m.rank, cm.rank));
                }
                Ok(result)
            })
        })
        .await;

    if let Err(e) = ship_data {
        return build_response(Err(e));
    }
    let ship_data = ship_data.unwrap();
    let data = srv
        .map_galaxy_coord(&planet_coord, |spaceobj, galaxy| {
            Box::pin(async move {
                let SpaceObject::Planet(planet_id) = spaceobj else {
                    return Err(Errcode::NotAPlanet);
                };
                let planet = galaxy.planets.get(planet_id).unwrap();
                let mut result = BTreeMap::new();
                for (res, density) in planet.resources_density.iter() {
                    for (modtype, mod_rank, op_rank) in ship_data.iter() {
                        if !res.extractable(modtype, *op_rank) {
                            continue;
                        }
                        let rate = calc_extraction_rate(res, *mod_rank, *op_rank, *density);
                        if let Some(r) = result.get_mut(res) {
                            *r += rate;
                        } else {
                            result.insert(*res, rate);
                        }
                    }
                }
                apply_power_attribution(&power_attribution, &mut result);
                Ok(to_value(result).unwrap())
            })
        })
        .await;
    build_response(data)
}

#[derive(Serialize, Deserialize)]
pub struct MiningRateComputation {
    module_type: ShipModuleType,
    module_rank: u8,
    operator_rank: u8,
    planet_coord: SpaceCoord,
    #[serde(default)]
    power_attribution: BTreeMap<Resource, u8>,
}

// @summary Compute how much resources can be extracted from a given planet
// @returns The mining rate for each resources
#[web::post("/mining_rate")]
async fn compute_mining_rate(
    srv: GameState,
    body: Json<MiningRateComputation>,
) -> impl web::Responder {
    let MiningRateComputation {
        module_type,
        module_rank,
        operator_rank,
        planet_coord,
        power_attribution,
    } = body.0;
    let data = srv
        .map_galaxy_coord(&planet_coord, |spaceobj, galaxy| {
            Box::pin(async move {
                let SpaceObject::Planet(planet_id) = spaceobj else {
                    return Err(Errcode::NotAPlanet);
                };
                let planet = galaxy.planets.get(planet_id).unwrap();
                let mut result = BTreeMap::new();
                for (res, density) in planet.resources_density.iter() {
                    if !res.extractable(&module_type, operator_rank) {
                        continue;
                    }
                    let rate = calc_extraction_rate(res, module_rank, operator_rank, *density);
                    result.insert(*res, rate);
                }
                apply_power_attribution(&power_attribution, &mut result);
                Ok(to_value(result).unwrap())
            })
        })
        .await;
    build_response(data)
}

#[derive(Serialize, Deserialize)]
pub struct UpgradeModule {
    module_type: ShipModuleType,
    module_start_rank: u8,
    module_end_rank: u8,
    crew_start_rank: u8,
    crew_end_rank: u8,
}

// @summary Computes the costs of upgrading a ship's module & operator, and its benefits on extraction rates
// @returns The price of the module upgrade, the crew upgrade, the increase in wages, and the diff in extraction rate (for a fixed planet resource density)
#[web::post("/upgrade/module")]
async fn compute_upgrade_module(body: Json<UpgradeModule>) -> impl web::Responder {
    let UpgradeModule {
        module_type,
        module_start_rank,
        module_end_rank,
        crew_start_rank,
        crew_end_rank,
    } = body.0;

    let mut start_module = ShipModule::new(module_type, module_start_rank);
    let mut operator = CrewMember::from(CrewMemberType::Operator);
    operator.rank = crew_start_rank;
    let start_wage = operator.wage();

    let mut upg_mod_price = 0.0;
    for r in module_start_rank..module_end_rank {
        start_module.rank = r;
        upg_mod_price += start_module.price_next_rank();
    }

    let mut upg_crew_price = 0.0;
    for r in crew_start_rank..crew_end_rank {
        operator.rank = r;
        upg_crew_price += operator.price_next_rank();
    }
    operator.rank = crew_end_rank;
    let end_wage = operator.wage();

    let mut extraction_rates = BTreeMap::new();
    for res in Resource::iter() {
        let start_rate = if res.extractable(&module_type, crew_start_rank) {
            calc_extraction_rate(&res, module_start_rank, crew_start_rank, 1.0)
        } else {
            0.0
        };

        if res.extractable(&module_type, crew_end_rank) {
            let end_rate = calc_extraction_rate(&res, module_end_rank, crew_end_rank, 1.0);
            extraction_rates.insert(res, end_rate - start_rate);
        };
    }

    build_response(Ok(json!({
        "diff_extraction_rate": to_value(extraction_rates).unwrap(),
        "module_upgrade_price": to_value(upg_mod_price).unwrap(),
        "crew_upgrade_price": to_value(upg_crew_price).unwrap(),
        "wage_increase": to_value(end_wage - start_wage).unwrap(),
    })))
}

#[derive(Serialize, Deserialize)]
pub struct UpgradeShip {
    start_reactor_rank: u16,
    end_reactor_rank: u16,

    start_shield_rank: u16,
    end_shield_rank: u16,

    start_pilot_rank: u8,
    end_pilot_rank: u8,

    start_cargo_capacity: f64,
    end_cargo_capacity: f64,
}

// @summary Computes the costs of upgrading a ship, and its benefits on the ship's statistics
// @returns The price of the pilot upgrade, the price of the reactor upgrade, the price of the shields upgrade, the increase in wages, and the ship statistics before / after upgrading
#[web::post("/upgrade/ship")]
async fn compute_upgrade_ship(body: Json<UpgradeShip>) -> impl web::Responder {
    let UpgradeShip {
        start_pilot_rank,
        end_pilot_rank,
        start_reactor_rank,
        end_reactor_rank,
        start_shield_rank,
        end_shield_rank,
        start_cargo_capacity,
        end_cargo_capacity,
    } = body.0;
    let mut pilot = CrewMember::from(CrewMemberType::Pilot);
    pilot.rank = start_pilot_rank;
    let start_wage = pilot.wage();
    let start_stats = ShipStats::new(
        start_reactor_rank,
        start_shield_rank,
        start_cargo_capacity,
        Some(&pilot),
    );

    let mut rpow_upgrade_price = 0.0;
    for _ in start_reactor_rank..end_reactor_rank {
        rpow_upgrade_price += ShipUpgrade::Reactor.get_price();
    }
    let mut spow_upgrade_price = 0.0;
    for _ in start_shield_rank..end_shield_rank {
        spow_upgrade_price += ShipUpgrade::Shield.get_price();
    }

    let cargo_upgrade_price =
        ShipExpandUpgrade::Cargo(end_cargo_capacity - start_cargo_capacity).get_price();
    let mut crew_upgrade_price = 0.0;
    for r in start_pilot_rank..end_pilot_rank {
        pilot.rank = r;
        crew_upgrade_price += pilot.price_next_rank();
    }
    pilot.rank = end_pilot_rank;
    let end_wage = pilot.wage();
    let end_stats = ShipStats::new(
        end_reactor_rank,
        end_shield_rank,
        end_cargo_capacity,
        Some(&pilot),
    );
    build_response(Ok(json!({
        "wage_increase": end_wage - start_wage,
        "pilot_upgrade_price": crew_upgrade_price,
        "reactor_upgrade_price": rpow_upgrade_price,
        "shield_upgrade_price": spow_upgrade_price,
        "cargo_upgrade_price": cargo_upgrade_price,
        "stats_before": start_stats,
        "stats_after": end_stats,
    })))
}

#[derive(Serialize, Deserialize)]
pub struct UpgradeTrader {
    trader_start_rank: u8,
    trader_end_rank: u8,
}

// @summary Computes the costs of upgrading a station's trader, and the new fee rate
// @returns The price of the upgrade, the increase in wages, the old and new fee rate
#[web::post("/upgrade/trader")]
async fn compute_upgrade_trader(body: Json<UpgradeTrader>) -> impl web::Responder {
    let UpgradeTrader {
        trader_start_rank,
        trader_end_rank,
    } = body.0;
    let mut trader = CrewMember::from(CrewMemberType::Trader);
    trader.rank = trader_start_rank;
    let start_wage = trader.wage();
    let start_fee = fee_rate(trader_start_rank);
    let end_fee = fee_rate(trader_end_rank);
    let mut crew_upgrade_price = 0.0;
    for r in trader_start_rank..trader_end_rank {
        trader.rank = r;
        crew_upgrade_price += trader.price_next_rank();
    }
    trader.rank = trader_end_rank;
    let end_wage = trader.wage();

    build_response(Ok(json!({
        "wage_before": start_wage,
        "fee_before": start_fee,
        "wage_after": end_wage,
        "fee_after": end_fee,
        "trader_upgrade_price": crew_upgrade_price,
    })))
}

#[derive(Serialize, Deserialize)]
pub struct ComputeShipWages {
    pilot_rank: Option<u8>,
    operator_ranks: Vec<u8>,
}

// @summary Compute the total wages for a ship with the given crew onboard
// @returns The total wage for the given crew on a ship
#[web::post("/wages/ship")]
async fn compute_ship_wages(body: Json<ComputeShipWages>) -> impl web::Responder {
    let mut total_wages = 0.0;
    if let Some(pilotrank) = body.pilot_rank {
        let mut cm = CrewMember::from(CrewMemberType::Pilot);
        cm.rank = pilotrank;
        total_wages += cm.wage();
    }
    for oprank in body.operator_ranks.iter() {
        let mut cm = CrewMember::from(CrewMemberType::Operator);
        cm.rank = *oprank;
        total_wages += cm.wage();
    }
    build_response(Ok(json!({
        "total_wages": total_wages,
    })))
}

// TODO (#44) Endpoints for all kinds of computations
// - Prices of the upgrade of an industry + its operator (from source lvl to dst lvl), returns benefits, price, wages, etc...
// - Resource requirements / production of an industry at a certain level

pub fn configure<T: IntoPattern>(base: T, srv: &mut ServiceConfig) {
    srv.service(
        scope(base)
            .service(compute_travel_costs)
            .service(compute_ship_wages)
            .service(compute_ship_mining_rate)
            .service(compute_mining_rate)
            .service(compute_upgrade_ship)
            .service(compute_upgrade_trader)
            .service(compute_upgrade_module),
    );
}
