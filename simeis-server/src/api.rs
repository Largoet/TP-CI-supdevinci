use ntex::web::{HttpResponse, ServiceConfig};
use serde_json::{json, Value};
use simeis_data::errors::Errcode;

use crate::GameState;

pub type ApiResult = Result<Value, Errcode>;

macro_rules! got_admin_token {
    ($req:ident, $tok:ident) => {
        'tok: {
            let Some(tok) = $req.headers().get("Simeis-Admin-Token") else {
                break 'tok false;
            };
            let admin_tok: &str = $tok.as_ref();
            if admin_tok != tok {
                log::warn!("Invalid admin token");
                break 'tok false;
            } else {
                break 'tok true;
            }
        }
    };
}

macro_rules! get_player_key {
    ($req:ident) => {
        'getk: {
            use base64::{prelude::BASE64_STANDARD, Engine};
            let Some(b64key) = $req.headers().get("Simeis-Key") else {
                return build_response(Err(Errcode::NoPlayerKey));
            };
            let mut key = [0; 128];
            if !BASE64_STANDARD
                .decode_slice(b64key, &mut key)
                .ok()
                .is_some()
            {
                return build_response(Err(Errcode::InvalidPlayerKey));
            };
            break 'getk key;
        }
    };
}

#[inline]
pub fn jsonmerge(a: &mut Value, b: &Value) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                jsonmerge(a.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (a, b) => *a = b.clone(),
    }
}

#[inline]
fn build_response(res: ApiResult) -> HttpResponse {
    let body = match res {
        Ok(mut data) => {
            jsonmerge(&mut data, &json!({"error": "ok"}));
            data
        }
        Err(e) => {
            json!({"error": e.errmsg(), "type": format!("{e:?}")})
        }
    };

    HttpResponse::Ok()
        .content_type("application/json")
        .json(&body)
}

mod admin;
mod market;
mod player;
mod ship;
mod station;
mod system;

// Nested
mod calc;
mod crew;
mod industry;
mod shipyard;
mod station_shop;

// TODO (#37) Document greatly the API

pub fn configure(srv: &mut ServiceConfig) {
    system::configure(srv);
    market::configure("/market", srv);
    player::configure("/player", srv);
    ship::configure("/ship/{ship_id}", srv);
    station::configure("/station/{station_id}", srv);
    calc::configure("/calc", srv);
    admin::configure("/admin", srv);
}
