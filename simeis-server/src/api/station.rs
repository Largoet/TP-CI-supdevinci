use ntex::router::IntoPattern;
use ntex::web;
use ntex::web::scope;
use ntex::web::types::Path;
use ntex::web::HttpRequest;
use ntex::web::ServiceConfig;

use serde_json::json;
use serde_json::to_value;
use serde_json::Value;

use simeis_data::errors::Errcode;
use simeis_data::galaxy::station::StationId;
use simeis_data::history::PlayerHistoryEvent;
use simeis_data::ship::ShipId;

use crate::api::build_response;
use crate::api::GameState;
use crate::utils::get_query_string_element;
use crate::AdminToken;

// @summary Get status of a station
// @returns All the informations for the player on this station
#[web::get("")]
async fn get_station_status(
    srv: GameState,
    id: Path<StationId>,
    req: HttpRequest,
    tok: AdminToken,
) -> impl web::Responder {
    if got_admin_token!(req, tok) {
        let qry = req.query_string();
        let Some(player_id) = get_query_string_element(qry, "player") else {
            return build_response(Err(Errcode::MissingQueryArgument("player")));
        };
        let Ok(player_id) = player_id.parse() else {
            return build_response(Err(Errcode::InvalidArgument("player_id")));
        };
        let data = srv
            .map_player_by_id(&player_id, |p| {
                Box::pin(async move {
                    let Some(station) = p.stations.get(&id) else {
                        return Err(Errcode::NoSuchStation(*id));
                    };
                    Ok(station.to_json(&player_id).await)
                })
            })
            .await;
        return build_response(data);
    }

    let id = id.as_ref();
    let key = get_player_key!(req);
    let data = srv
        .map_station(&key, id, |pid, _, station| {
            Box::pin(async move { Ok(station.to_json(&pid).await) })
        })
        .await;
    build_response(data)
}

// @summary Scan for planets around the station
// @returns Scan information on all the stellar objects around this station
#[web::post("/scan")]
async fn scan(id: Path<StationId>, srv: GameState, req: HttpRequest) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let station_id = *id;

    let data = srv
        .scan_galaxy(&pkey, &station_id)
        .await
        .map(|v| to_value(v).unwrap());
    build_response(data)
}

// @summary List the wages for all the crew assigned on this station
// @returns List of the wages by crew assigned
#[web::get("/wages")]
async fn get_station_wages(
    srv: GameState,
    args: Path<StationId>,
    req: HttpRequest,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let station_id = *args;

    let data = srv
        .map_station(&pkey, &station_id, |pid, _, station| {
            Box::pin(async move {
                let Some(pd) = station.player_data.clone_val(&pid).await else {
                    return Ok(json!({}));
                };
                let pd = pd.read().await;
                let mut data = serde_json::Map::new();
                if let Some(id) = pd.trader {
                    let cm = pd.crew.0.get(&id).unwrap();
                    data.insert(
                        "trader".into(),
                        json!({
                            "id": id,
                            "wage": cm.wage(),
                            "rank": cm.rank,
                        }),
                    );
                }
                let mut industry_data = serde_json::Map::new();
                for (id, unit) in pd.industry.iter() {
                    let Some(op) = unit.operator else {
                        continue;
                    };
                    let cm = pd.crew.0.get(&op).unwrap();
                    industry_data.insert(
                        id.to_string(),
                        json!({
                            "operator": op,
                            "wage": cm.wage(),
                            "rank": cm.rank,
                        }),
                    );
                }
                data.insert("industry".into(), Value::Object(industry_data));
                Ok(Value::Object(data))
            })
        })
        .await;
    build_response(data)
}

// @summary List the upgrades for a station currently available
// @returns List of the upgrades available, and their price
#[web::get("/upgrades")]
async fn get_station_upgrades(
    srv: GameState,
    args: Path<StationId>,
    req: HttpRequest,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let station_id = *args;

    let data = srv
        .map_station(&pkey, &station_id, |pid, _, station| {
            Box::pin(async move {
                let cargoprice = station.cargo_price(&pid).await;
                let traderprice = station.upgr_trader_price(&pid).await;
                Ok(json!({
                    "cargo": cargoprice,
                    "trader": traderprice,
                }))
            })
        })
        .await;
    build_response(data)
}

// @summary Use fuel in storage on the station to refuel the ship
// @returns How much fuel was effectively added to the ship (and removed from the station cargo)
#[web::post("/refuel/{ship_id}")]
async fn refuel_ship(
    srv: GameState,
    args: Path<(StationId, ShipId)>,
    req: HttpRequest,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let (station_id, ship_id) = *args;

    let data = srv
        .map_ship_mut_in_station(&pkey, &station_id, &ship_id, |_, hist, station, ship| {
            Box::pin(async move {
                let added = station.refuel_ship(ship).await?;
                let tank_full = (ship.fuel_tank - ship.fuel_tank_capacity).abs() < 0.0000001;
                hist.add(PlayerHistoryEvent::RefuelShip {
                    station: station_id,
                    ship: ship_id,
                    amnt: added,
                    tank_full,
                })
                .await;
                Ok(json!({ "added-fuel": added }))
            })
        })
        .await;
    build_response(data)
}

// @summary Use the hull plates in storage on the station to repair the ship
// @returns How much hull was effectively used to repair the ship (and removed from the station cargo)
#[web::post("/repair/{ship_id}")]
async fn repair_ship(
    srv: GameState,
    args: Path<(StationId, ShipId)>,
    req: HttpRequest,
) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let (station_id, ship_id) = *args;
    let data = srv
        .map_ship_mut_in_station(&pkey, &station_id, &ship_id, |_, hist, station, ship| {
            Box::pin(async move {
                let added = station.repair_ship(ship).await?;
                let fully_repaired = ship.hull_decay < 0.0000001;
                hist.add(PlayerHistoryEvent::RepairedShip {
                    station: station_id,
                    ship: ship_id,
                    amnt: added,
                    fully_repaired,
                })
                .await;
                Ok(json!({"added-hull": added }))
            })
        })
        .await;
    build_response(data)
}

pub fn configure<T: IntoPattern>(base: T, srv: &mut ServiceConfig) {
    srv.service(
        scope(base)
            .configure(|srv| crate::api::shipyard::configure("/shipyard", srv))
            .configure(|srv| crate::api::crew::configure("/crew", srv))
            .configure(|srv| crate::api::station_shop::configure("/shop", srv))
            .configure(|srv| crate::api::industry::configure("/industry", srv))
            .service(scan)
            .service(get_station_status)
            .service(get_station_wages)
            .service(get_station_upgrades)
            .service(refuel_ship)
            .service(repair_ship),
    );
}
