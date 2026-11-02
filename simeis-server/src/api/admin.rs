use std::time::Duration;

use ntex::router::IntoPattern;
use ntex::web::types::Path;
use ntex::web::{self, scope, HttpRequest, ServiceConfig};
use serde_json::json;
use simeis_data::errors::Errcode;
use simeis_data::galaxy::planet::{PlanetId, PlanetInfo};
use simeis_data::galaxy::station::StationId;
use simeis_data::player::PlayerId;
use simeis_data::syslog::SyslogEvent;

use crate::api::build_response;
use crate::{AdminToken, GameState};

// @summary Know if we are admin or not
// @returns the boolean flag if we are admin or not
// @noswagger
#[web::get("")]
async fn is_admin(req: HttpRequest, tok: AdminToken) -> impl web::Responder {
    let is_admin = got_admin_token!(req, tok);
    build_response(Ok(json!({"is_admin": is_admin})))
}

// @summary Get the informations about a planet from its ID
// @returns Planet informations
// @noswagger
#[web::get("/planet/{id}")]
async fn get_planet(
    srv: GameState,
    id: Path<PlanetId>,
    req: HttpRequest,
    tok: AdminToken,
) -> impl web::Responder {
    if got_admin_token!(req, tok) {
        let data = srv
            .sync_map_planet_id(&id, |pla| {
                Ok(serde_json::to_value(PlanetInfo::scan(u8::MAX, pla)).unwrap())
            })
            .await;
        build_response(data)
    } else {
        build_response(Err(Errcode::NotAnAdmin))
    }
}

// @summary Get the list of all stations created on the server
// @returns List of station IDs
// @noswagger
#[web::get("/station/list")]
async fn get_station_list(
    srv: GameState,
    req: HttpRequest,
    tok: AdminToken,
) -> impl web::Responder {
    if got_admin_token!(req, tok) {
        let all_stations = srv.stations.read().await;
        let all_ids = all_stations.keys().cloned().collect::<Vec<StationId>>();
        build_response(Ok(json!({ "stations": all_ids })))
    } else {
        build_response(Err(Errcode::NotAnAdmin))
    }
}

// @summary Get the informations about a station and all its player data
// @returns Station informations
// @noswagger
#[web::get("/station/{id}")]
async fn get_station_admin(
    srv: GameState,
    id: Path<StationId>,
    req: HttpRequest,
    tok: AdminToken,
) -> impl web::Responder {
    if got_admin_token!(req, tok) {
        let all_stations = srv.stations.read().await;
        let id = *id;
        let Some(station) = all_stations.get(&id) else {
            return build_response(Err(Errcode::NoSuchStation(id)));
        };
        build_response(Ok(station.to_json_all_players().await))
    } else {
        build_response(Err(Errcode::NotAnAdmin))
    }
}

// @summary Get all the space objects in the sector of a specific station
// @returns Space objects
// @noswagger
#[web::get("/station/{id}/around")]
async fn get_space_objects_around_station(
    srv: GameState,
    id: Path<StationId>,
    req: HttpRequest,
    tok: AdminToken,
) -> impl web::Responder {
    if got_admin_token!(req, tok) {
        let all_stations = srv.stations.read().await;
        let id = *id;
        let Some(station) = all_stations.get(&id) else {
            return build_response(Err(Errcode::NoSuchStation(id)));
        };
        let galaxy = srv.galaxy.read().await;
        let objects = station.scan(&galaxy);
        build_response(Ok(serde_json::to_value(objects).unwrap()))
    } else {
        build_response(Err(Errcode::NotAnAdmin))
    }
}

// @summary Removes a player by his ID
// @returns Nothing
// @noswagger
#[web::post("/player/remove/{id}")]
async fn remove_player(
    srv: GameState,
    id: Path<PlayerId>,
    req: HttpRequest,
    tok: AdminToken,
) -> impl web::Responder {
    if got_admin_token!(req, tok) {
        let data = srv.remove_player(&id).await.map(|_| json!({}));
        build_response(data)
    } else {
        build_response(Err(Errcode::NotAnAdmin))
    }
}

// @summary Resets the game completely
// @returns Nothing
// @noswagger
#[web::post("/reset")]
async fn reset_game(srv: GameState, req: HttpRequest, tok: AdminToken) -> impl web::Responder {
    if got_admin_token!(req, tok) {
        srv.reset().await;
        build_response(Ok(json!({})))
    } else {
        build_response(Err(Errcode::NotAnAdmin))
    }
}

// @summary Resets the game completely in N seconds
// @returns Nothing
// @noswagger
#[web::post("/reset/{n}")]
async fn reset_game_cooldown(
    srv: GameState,
    n: Path<u64>,
    req: HttpRequest,
    tok: AdminToken,
) -> impl web::Responder {
    if got_admin_token!(req, tok) {
        ntex::rt::spawn(async move {
            let mut n = *n;
            srv.notify_all_players(SyslogEvent::ResetIn(n)).await;
            while n > 10 {
                crate::utils::sleep(Duration::from_secs(10)).await;
                n -= 10;
                srv.notify_all_players(SyslogEvent::ResetIn(n)).await;
            }
            if n > 0 {
                crate::utils::sleep(Duration::from_secs(n)).await;
            }
            srv.reset().await;
        });
        build_response(Ok(json!({})))
    } else {
        build_response(Err(Errcode::NotAnAdmin))
    }
}

pub fn configure<T: IntoPattern>(base: T, srv: &mut ServiceConfig) {
    srv.service(
        scope(base)
            .service(get_planet)
            .service(remove_player)
            .service(get_station_list)
            .service(get_station_admin)
            .service(get_space_objects_around_station)
            .service(reset_game)
            .service(is_admin)
            .service(reset_game_cooldown),
    );
}
