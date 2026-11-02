use std::collections::BTreeMap;

use ntex::router::IntoPattern;
use ntex::web;
use ntex::web::types::{Json, Path};
use ntex::web::{scope, HttpRequest, ServiceConfig};

use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::to_value;
use simeis_data::config::get_config;
use simeis_data::crew::{CrewId, CrewMemberType};
use simeis_data::galaxy::SpaceUnit;
use simeis_data::history::PlayerHistoryEvent;
use simeis_data::ship::module::ShipModuleId;
use simeis_data::ship::resources::Resource;
use simeis_data::syslog::SyslogEvent;

use simeis_data::errors::Errcode;
use simeis_data::galaxy::station::StationId;
use simeis_data::ship::ShipId;
use strum::IntoEnumIterator;

use crate::api::build_response;
use crate::api::GameState;
use crate::AdminToken;

// @summary Get the status of the ship
// @returns The data for the ship
#[web::get("")]
async fn get_ship_status(
    srv: GameState,
    id: Path<ShipId>,
    req: HttpRequest,
    tok: AdminToken,
) -> impl web::Responder {
    if got_admin_token!(req, tok) {
        let Some(owner) = srv.get_ship_owner(&id).await else {
            return build_response(Err(Errcode::ShipNotFound(*id)));
        };
        let data = srv
            .sync_map_player_by_id(&owner, |p, _| {
                p.export_ship_json(&id).ok_or(Errcode::ShipNotFound(*id))
            })
            .await;
        return build_response(data);
    }

    let pkey = get_player_key!(req);
    let ship_id = *id;
    let data = srv
        .map_ship(&pkey, &ship_id, |_, _, ship| {
            Box::pin(async move { Ok(to_value(ship).unwrap()) })
        })
        .await;
    build_response(data)
}

// @summary Reassigns an operator from one module to another
// @returns Nothing
#[web::post("/reassign/{operator}/{module}")]
async fn reassign_operator(
    srv: GameState,
    args: Path<(ShipId, CrewId, ShipModuleId)>,
    req: HttpRequest,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let (ship_id, crew_id, module_id) = *args;

    let data = srv
        .map_ship_mut(&pkey, &ship_id, |_, _, ship| {
            Box::pin(async move {
                ship.reassign_operator(crew_id, module_id)
                    .map(|_| json!({}))
            })
        })
        .await;
    build_response(data)
}

// @summary Compute how much wages cost per second for this ship
// @returns The amount of credits consumed each second by the crew of this ship
#[web::get("/wages")]
async fn get_wages_cost(
    srv: GameState,
    args: Path<ShipId>,
    req: HttpRequest,
) -> impl web::Responder {
    let ship_id = *args;
    let pkey = get_player_key!(req);

    let data = srv
        .map_ship(&pkey, &ship_id, |_, _, ship| {
            Box::pin(async move {
                let mut s = 0.0;
                let mut pilot = None;
                let mut operators = serde_json::Map::new();
                for (id, cm) in ship.crew.0.iter() {
                    let w = cm.wage();
                    s += w;
                    if cm.member_type == CrewMemberType::Pilot {
                        pilot = Some(w);
                    } else if cm.member_type == CrewMemberType::Operator {
                        operators.insert(id.to_string(), serde_json::to_value(w).unwrap());
                    } else {
                        unreachable!()
                    }
                }
                Ok(json!({ "total": s, "pilot": pilot, "operators": operators }))
            })
        })
        .await;
    build_response(data)
}

#[derive(Serialize, Deserialize)]
pub struct NavigationDestination {
    x: SpaceUnit,
    y: SpaceUnit,
    z: SpaceUnit,
}

// @summary Navigate to position (X, Y, Z)
// @postbody Coordinate of the destination, in X, Y and Z
// @returns Travel informations on the destination
// Ship will have the state InFlight during the travel
#[web::post("/navigate")]
async fn ask_navigate(
    srv: GameState,
    id: Path<ShipId>,
    body: Json<NavigationDestination>,
    req: HttpRequest,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let NavigationDestination { x, y, z } = body.0;
    let id = id.into_inner();
    let data = srv
        .map_ship_mut(&pkey, &id, |_, hist, ship| {
            Box::pin(async move {
                let src = ship.position;
                let cost = ship.set_travel((x, y, z))?;
                hist.add(PlayerHistoryEvent::ShipNavigate {
                    dest: (x, y, z),
                    fuel_cost: cost.fuel_consumption,
                    hull_usage: cost.hull_usage,
                    duration_secs: cost.duration,
                    ship: id,
                    src,
                })
                .await;
                Ok(to_value(cost).unwrap())
            })
        })
        .await;
    build_response(data)
}

// @summary Stop the naviguation, ship will become Idle, and stay in place
// @returns The position where the ship has stopped
#[web::post("/navigation/stop")]
async fn stop_navigation(
    srv: GameState,
    args: Path<ShipId>,
    req: HttpRequest,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let id = *args;
    let data = srv
        .map_ship_mut(&pkey, &id, |_, hist, ship| {
            Box::pin(async move {
                let pos = ship.stop_navigation()?;
                hist.add(PlayerHistoryEvent::StopNavigation {
                    ship: id,
                    position: pos,
                })
                .await;
                Ok(json!({ "position": pos }))
            })
        })
        .await;
    build_response(data)
}

// @summary Start the extraction of resources on the planet
// @returns The rate at which every resources are mined, and the time necessary to fill the cargo
// Ship will have the state "Extracting" until its cargo is full
#[web::post("/extraction/start")]
async fn start_extraction(
    srv: GameState,
    id: Path<ShipId>,
    req: HttpRequest,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let ship_id = *id;
    let data = srv
        .start_player_extraction(&pkey, &ship_id)
        .await
        .map(|v| to_value(v).unwrap());
    build_response(data)
}

// @summary Stop the extraction of resources on the planet, the ship state will be Idle
// @returns Nothing
#[web::post("/extraction/stop")]
async fn stop_extraction(
    srv: GameState,
    id: Path<ShipId>,
    req: HttpRequest,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let ship_id = *id;
    let data = srv
        .map_player_mut(&pkey, |player| {
            Box::pin(async move {
                let history = player.history.clone();
                let ship = player.get_ship_mut(&ship_id)?;
                ship.stop_extraction()?;
                history
                    .add(PlayerHistoryEvent::StopExtraction {
                        ship: ship_id,
                        cargo_usage_perc: ship.cargo.usage / ship.cargo.capacity,
                    })
                    .await;
                Ok(json!({}))
            })
        })
        .await;
    build_response(data)
}

// @summary Get the attribution of power for all the mineable resources by the ship
// The default power value is 128
// The maximum power value is 255
// The minimum power value is 0
// @returns The current power attribution set for all mineable resources by the ship
#[web::get("/extraction/power_attribution")]
async fn get_extraction_power_attribution(
    srv: GameState,
    id: Path<ShipId>,
    req: HttpRequest,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let ship_id = *id;
    let data = srv
        .map_player(&pkey, |player| {
            Box::pin(async move {
                let ship = player.get_ship(&ship_id)?;
                let mut result = BTreeMap::new();
                for res in Resource::iter() {
                    for smod in ship.modules.values() {
                        let Some(opid) = smod.operator else {
                            continue;
                        };
                        let opcrew = ship.crew.0.get(&opid).unwrap();
                        if res.extractable(&smod.modtype, opcrew.rank) {
                            let cfg = get_config();
                            let powattr = ship
                                .power_attribution
                                .get(&res)
                                .unwrap_or(&cfg.ship.default_pow_attr);
                            result.insert(res, *powattr);
                        }
                    }
                }
                Ok(json!({"power_attribution": result }))
            })
        })
        .await;
    build_response(data)
}

// @summary Set the attribution of power for a specific mineable resource
// The default power value is 128
// The maximum power value is 255
// The minimum power value is 0
// Setting the power attribution for a non-mineable resource will have no effect
// @returns Nothing
#[web::post("/extraction/power_attribution/{resource}/{pow}")]
async fn set_extraction_power_attribution(
    srv: GameState,
    args: Path<(ShipId, Resource, u8)>,
    req: HttpRequest,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let (ship_id, res, pow) = *args;
    let data = srv
        .map_player_mut(&pkey, |player| {
            Box::pin(async move {
                let ship = player.get_ship_mut(&ship_id)?;
                ship.power_attribution.insert(res, pow);
                Ok(json!({}))
            })
        })
        .await;
    build_response(data)
}

// @summary Unload the whole cargo of a ship into a station
// @returns How much of each resource were unloaded, and if the ship cargo is empty or not
#[web::post("/unload/{station_id}/all")]
async fn unload_all_ship_cargo(
    srv: GameState,
    args: Path<(ShipId, StationId)>,
    req: HttpRequest,
) -> impl web::Responder {
    let (ship_id, station_id) = *args;
    let pkey = get_player_key!(req);
    let data = srv
        .map_ship_mut_in_station(&pkey, &station_id, &ship_id, |_, hist, station, ship| {
            Box::pin(async move {
                let unloaded = ship.unload_all(station).await?;
                let emptied = ship.cargo.usage < 0.0000001;
                hist.add(PlayerHistoryEvent::UnloadShip {
                    ship: ship_id,
                    station: station_id,
                    emptied,
                    resources: unloaded.keys().cloned().collect(),
                })
                .await;
                Ok(json!({
                    "unloaded": unloaded,
                    "emptied": emptied,
                }))
            })
        })
        .await;
    build_response(data)
}

// @summary Unload a specific amount of a specific resource on the station's storage
// @returns How much of this resource was effectively unloaded from the ship
// Depending on the cargo space available on the station, may not unload anything
#[web::post("/unload/{station_id}/{resource}/{amnt}")]
async fn unload_ship_cargo(
    srv: GameState,
    args: Path<(ShipId, StationId, Resource, f64)>,
    req: HttpRequest,
) -> impl web::Responder {
    let (ship_id, station_id, resource, amnt) = *args;
    let pkey = get_player_key!(req);

    let data = srv
        .map_ship_mut_in_station(&pkey, &station_id, &ship_id, |_, hist, station, ship| {
            Box::pin(async move {
                let unloaded = ship.unload_cargo(&resource, amnt, station).await?;
                let emptied = ship.cargo.usage < 0.0000001;
                hist.add(PlayerHistoryEvent::UnloadShip {
                    ship: ship_id,
                    station: station_id,
                    emptied,
                    resources: vec![resource],
                })
                .await;
                Ok(unloaded)
            })
        })
        .await;

    if let Ok(0.0) = data {
        let (pid, ev) = srv
            .map_ship_in_station(&pkey, &station_id, &ship_id, |pid, _, station, ship| {
                Box::pin(async move {
                    Ok((
                        pid,
                        SyslogEvent::UnloadedNothing {
                            station_cargo: station.clone_cargo(&pid).await,
                            ship_cargo: ship.cargo.clone(),
                        },
                    ))
                })
            })
            .await
            .unwrap();
        srv.syslog.event(&pid, ev).await;
    }
    build_response(data.map(|v| json!({ "unloaded": v })))
}

pub fn configure<T: IntoPattern>(base: T, srv: &mut ServiceConfig) {
    srv.service(
        scope(base)
            .service(get_wages_cost)
            .service(get_ship_status)
            .service(reassign_operator)
            .service(ask_navigate)
            .service(stop_navigation)
            .service(start_extraction)
            .service(stop_extraction)
            .service(get_extraction_power_attribution)
            .service(set_extraction_power_attribution)
            .service(unload_all_ship_cargo)
            .service(unload_ship_cargo),
    );
}
