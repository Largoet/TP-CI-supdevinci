use std::collections::BTreeMap;

use ntex::router::IntoPattern;
use ntex::web;
use ntex::web::scope;
use ntex::web::types::Path;
use ntex::web::HttpRequest;
use ntex::web::ServiceConfig;

use serde_json::json;
use simeis_data::player::PlayerId;

use simeis_data::errors::Errcode;

use crate::api::build_response;
use crate::api::GameState;
use crate::utils::get_query_string_element;
use crate::AdminToken;

// @summary Creates a new player in the game
// @returns The ID of the player, and its authentication secret key
// The secret key must be used in the "Simeis-Key" HTTP header
#[web::post("/new/{name}")]
async fn new_player(srv: GameState, name: Path<String>) -> impl web::Responder {
    let name = name.to_string();
    let res = srv.new_player(name).await.map(|(id, key)| {
        json!({
            "playerId": id,
            "key": key,
        })
    });
    build_response(res)
}

// @summary Get the status from the player of a given id.
// @returns Information about the player, complete if it's you, minimal if it's another player
#[web::get("/{player_id}")]
async fn get_player(
    srv: GameState,
    tok: AdminToken,
    id: Path<PlayerId>,
    req: HttpRequest,
) -> impl web::Responder {
    if got_admin_token!(req, tok) {
        let data = srv.export_player_data(&id, true).await;
        return build_response(data);
    }

    let pkey = get_player_key!(req);
    let id = id.as_ref();
    let data = srv.player_to_json(&pkey, id).await;
    build_response(data)
}

// @summary Get the detailled wages for a player
// @returns For each crew member, its  type, where it's assigned, and his wage
#[web::get("/wages")]
async fn get_all_wages(srv: GameState, req: HttpRequest) -> impl web::Responder {
    let pkey = get_player_key!(req);
    let data = srv
        .map_player(&pkey, |player| {
            Box::pin(async move {
                let mut result = BTreeMap::new();
                for (sid, station) in player.stations.iter() {
                    let Some(pd) = station.player_data.clone_val(&player.id).await else {
                        continue;
                    };
                    let pd = pd.read().await;
                    for (id, cm) in pd.crew.0.iter() {
                        result.insert(
                            *id,
                            json!({
                                "crew_type": cm.member_type,
                                "assigned_on": "station",
                                "assigned_id": sid,
                                "wage": cm.wage(),
                                "rank": cm.rank,
                            }),
                        );
                    }
                }

                for (sid, ship) in player.ships.iter() {
                    for (id, cm) in ship.crew.0.iter() {
                        result.insert(
                            *id,
                            json!({
                                "crew_type": cm.member_type,
                                "assigned_on": "ship",
                                "assigned_id": sid,
                                "wage": cm.wage(),
                                "rank": cm.rank,
                            }),
                        );
                    }
                }
                Ok(json!(result))
            })
        })
        .await;
    build_response(data)
}

// @summary Get the history of the last 100 actions performed by the player
// @returns List of the actions, with associated timestamps
#[web::get("/history")]
async fn get_history(srv: GameState, tok: AdminToken, req: HttpRequest) -> impl web::Responder {
    let data = if got_admin_token!(req, tok) {
        let qry = req.query_string();
        let Some(player_id) = get_query_string_element(qry, "player") else {
            return build_response(Err(Errcode::MissingQueryArgument("player")));
        };
        let Ok(player_id) = player_id.parse() else {
            return build_response(Err(Errcode::InvalidArgument("player_id")));
        };
        srv.map_player_by_id(&player_id, |player| {
            Box::pin(async move {
                let hist = player.history.export().await;
                Ok(json!({ "history": hist }))
            })
        })
        .await
    } else {
        let pkey = get_player_key!(req);
        srv.map_player(&pkey, |player| {
            Box::pin(async move {
                let hist = player.history.export().await;
                Ok(json!({ "history": hist }))
            })
        })
        .await
    };
    build_response(data)
}

pub fn configure<T: IntoPattern>(base: T, srv: &mut ServiceConfig) {
    srv.service(
        scope(base)
            .service(get_history)
            .service(get_all_wages)
            .service(new_player)
            .service(get_player),
    );
}
