use serde_json::{Value, json};

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Duration;

// From the market transaction, returns:
// - How much money we spent
// - How much resource we bought
pub fn tx_buy(tx: &Value) -> (f64, f64) {
    let got = json_get_float("removed_money", tx).unwrap();
    let amnt = json_get_list("added_cargo", tx)
        .unwrap()
        .get(1)
        .unwrap()
        .as_f64()
        .unwrap();
    (got, amnt)
}

// From the market transaction, returns:
// - How much money we gained
// - How much resource we sold
pub fn tx_sell(tx: &Value) -> (f64, f64) {
    let got = json_get_float("added_money", tx).unwrap();
    let amnt = json_get_list("removed_cargo", tx)
        .unwrap()
        .get(1)
        .unwrap()
        .as_f64()
        .unwrap();
    (got, amnt)
}

pub fn get_id(data: &Value) -> u64 {
    json_get_uint("id", data).unwrap()
}

pub fn get_dist(posa: (u64, u64, u64), posb: (u64, u64, u64)) -> f64 {
    let xd = (posa.0 as f64) - (posb.0 as f64);
    let yd = (posa.1 as f64) - (posb.1 as f64);
    let zd = (posa.2 as f64) - (posb.2 as f64);
    (xd.powf(2.0) + yd.powf(2.0) + zd.powf(2.0)).sqrt()
}

pub fn get_position(planet: &Value) -> Option<(u64, u64, u64)> {
    let v = json_get_list("position", planet)?;
    match (
        v.first().and_then(|n| n.as_u64()),
        v.get(1).and_then(|n| n.as_u64()),
        v.get(2).and_then(|n| n.as_u64()),
    ) {
        (Some(x), Some(y), Some(z)) => Some((x, y, z)),
        _ => None,
    }
}

#[inline]
pub fn json_get_key<'a>(key: &str, mut val: &'a Value) -> Option<&'a Value> {
    if !key.contains(".") {
        let obj = val.as_object()?;
        return obj.get(key);
    }
    for k in key.split(".") {
        let obj = val.as_object()?;
        val = obj.get(k)?;
    }
    Some(val)
}
#[inline]
pub fn json_get_list<'a>(key: &str, val: &'a Value) -> Option<Vec<&'a Value>> {
    let val = json_get_key(key, val)?.as_array()?;
    Some(val.iter().collect())
}
#[inline]
pub fn json_get_dict<'a>(key: &str, val: &'a Value) -> Option<HashMap<&'a String, &'a Value>> {
    let val = json_get_key(key, val)?.as_object()?;
    let mut res = HashMap::new();
    for (key, val) in val.iter() {
        res.insert(key, val);
    }
    Some(res)
}
#[inline]
pub fn json_get_float(key: &str, val: &Value) -> Option<f64> {
    json_get_key(key, val)?.as_f64()
}
#[inline]
pub fn json_get_string<'a>(key: &str, val: &'a Value) -> Option<&'a str> {
    json_get_key(key, val)?.as_str()
}
#[inline]
pub fn json_get_uint(key: &str, val: &Value) -> Option<u64> {
    json_get_key(key, val)?.as_u64()
}
#[inline]
pub fn json_get_bool(key: &str, val: &Value) -> Option<bool> {
    json_get_key(key, val)?.as_bool()
}

pub type ApiResult = Result<Value, Value>;

#[derive(Clone)]
pub struct SimeisSDK {
    url: String,
    player_id: Option<u64>,
    player_key: Option<String>,
    game_config: Option<serde_json::Value>,
}

impl std::fmt::Debug for SimeisSDK {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimeisSDK").finish()
    }
}

impl SimeisSDK {
    pub fn new<T: ToString>(username: String, ip: T, port: u16) -> SimeisSDK {
        let url = format!("http://{}:{port}", ip.to_string());
        let mut sdk = SimeisSDK {
            url,
            player_id: None,
            player_key: None,
            game_config: None,
        };
        assert!(sdk.ping(), "Unable to connect to server, ping failed");
        sdk.setup_player(username, false)
            .expect("Unable to setup player");
        let config = sdk.get_game_config().unwrap();
        sdk.game_config = Some(config);
        sdk
    }

    fn api<T: ToString>(&self, path: T, get: bool, body: Option<Value>) -> ApiResult {
        debug_assert!(path.to_string().starts_with("/"));

        let got = if get {
            let mut req = ureq::get(format!("{}{}", self.url, path.to_string()));
            if let Some(ref key) = self.player_key {
                req = req.header("Simeis-Key", key);
            }
            req.call()
        } else {
            let mut req = ureq::post(format!("{}{}", self.url, path.to_string()));
            if let Some(ref key) = self.player_key {
                req = req.header("Simeis-Key", key);
            }
            let mut data = vec![];
            if let Some(ref b) = body {
                req = req.header("Content-Type", "application/json");
                serde_json::to_writer(&mut data, b).unwrap()
            }
            req.send(&data)
        };
        if let Err(ref e) = got {
            match e {
                ureq::Error::StatusCode(408) => {
                    log::warn!("Timeout");
                    std::thread::sleep(Duration::from_millis(100));
                    return self.api(path, get, body);
                }
                ureq::Error::StatusCode(101) => {
                    log::warn!("Network unreachable");
                    std::thread::sleep(Duration::from_secs(2));
                    return self.api(path, get, body);
                }
                err => {
                    return Err(json!({
                        "status": 0,
                        "error": format!("{err:?}"),
                    }));
                }
            }
        }
        let data = got
            .unwrap()
            .body_mut()
            .read_to_string()
            .expect("Non textual data from server");
        let mut data: Value = serde_json::from_str(&data)
            .expect("Unable to decode JSON from what the server returned");
        let datamap = data.as_object_mut().unwrap();
        let err = datamap
            .remove("error")
            .expect("Missing error field in reply");
        if err != "ok" { Err(data) } else { Ok(data) }
    }

    pub fn get<T: ToString>(&self, path: T) -> ApiResult {
        log::debug!("GET {}", path.to_string());
        self.api(path, true, None)
    }

    pub fn post<T: ToString>(&self, path: T) -> ApiResult {
        log::debug!("POST {}", path.to_string());
        self.api(path, false, None)
    }

    pub fn post_with_body<T: ToString>(&self, path: T, body: Value) -> ApiResult {
        log::debug!("POST {} with body {body:?}", path.to_string());
        self.api(path, false, Some(body))
    }

    pub fn ping(&self) -> bool {
        let Ok(got) = self.get("/ping") else {
            return false;
        };
        matches!(json_get_string("ping", &got), Some("pong"))
    }

    fn setup_player(&mut self, mut username: String, force_register: bool) -> Result<(), Value> {
        username = username
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();

        let path = PathBuf::from(format!("./{username}.json"));

        let player = if !path.exists() || force_register {
            let player_tmp = self.post(format!("/player/new/{username}"))?;
            let player_json_str = serde_json::to_string(&player_tmp).unwrap();
            std::fs::write(&path, player_json_str).expect("Unable to write player data to path");
            player_tmp
        } else {
            let player_json_str =
                std::fs::read_to_string(&path).expect("Unable to read player data from path");
            serde_json::from_str(&player_json_str).expect("Unable to load player JSON data")
        };

        self.player_key = Some(json_get_string("key", &player).unwrap().to_string());
        self.player_id = Some(json_get_uint("playerId", &player).unwrap());

        let player_id = json_get_uint("playerId", &player).unwrap();
        std::thread::sleep(Duration::from_secs(1));
        let Ok(player_status) = self.get(format!("/player/{player_id}")) else {
            return self.setup_player(username, true);
        };

        let player_money = json_get_float("money", &player_status).unwrap();
        if player_money <= 0.0 {
            println!("!!! Player already lost, please restart the server to reset the game");
            std::process::exit(0);
        }
        Ok(())
    }

    pub fn get_player_status(&self) -> ApiResult {
        self.get(format!("/player/{}", self.player_id.unwrap()))
    }

    pub fn get_ship_status(&self, ship_id: u64) -> ApiResult {
        self.get(format!("/ship/{ship_id}"))
    }

    pub fn get_station_status(&self, station_id: u64) -> ApiResult {
        self.get(format!("/station/{station_id}"))
    }

    pub fn shop_list_modules(&self, station_id: u64) -> Result<HashMap<String, Value>, Value> {
        let all = self.get(format!("/station/{station_id}/shop/modules"))?;
        let mut result = HashMap::new();
        let Value::Object(data) = all else {
            unreachable!();
        };
        for (key, val) in data.into_iter() {
            result.insert(key, val);
        }
        Ok(result)
    }

    pub fn shop_list_ship(&self, station_id: u64) -> Result<Vec<Value>, Value> {
        let all = self.get(format!("/station/{station_id}/shipyard/list"))?;
        let Value::Object(mut omap) = all else {
            unreachable!();
        };
        let Some(Value::Array(mut allvec)) = omap.remove("ships") else {
            unreachable!();
        };
        allvec.sort_by(|a, b| {
            let pa = json_get_float("price", a).unwrap();
            let pb = json_get_float("price", b).unwrap();
            pa.partial_cmp(&pb).unwrap()
        });
        Ok(allvec)
    }

    pub fn list_station_upgrades(&self, station_id: u64) -> ApiResult {
        self.get(format!("/station/{station_id}/upgrades"))
    }

    pub fn buy_ship(&self, station_id: u64, ship_id: u64) -> ApiResult {
        self.post(format!("/station/{station_id}/shipyard/buy/{ship_id}"))
    }

    pub fn buy_module_on_ship(&self, station_id: u64, ship_id: u64, modtype: &str) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/shop/modules/{ship_id}/buy/{}",
            modtype.to_lowercase()
        ))
    }

    pub fn hire_crew(&self, station_id: u64, crewtype: &str) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/crew/hire/{}",
            crewtype.to_lowercase()
        ))
    }

    pub fn fire_crew(&self, station_id: u64, crewtype: &str) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/crew/fire/{}",
            crewtype.to_lowercase()
        ))
    }

    pub fn assign_crew_to_ship_module(
        &self,
        station_id: u64,
        ship_id: u64,
        crew_id: u64,
        mod_id: u64,
    ) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/crew/assign/{crew_id}/ship/{ship_id}/{mod_id}"
        ))
    }

    pub fn assign_crew_as_ship_pilot(
        &self,
        station_id: u64,
        ship_id: u64,
        pilot_id: u64,
    ) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/crew/assign/{pilot_id}/ship/{ship_id}/pilot"
        ))
    }

    pub fn station_has_trader(&self, station_id: u64) -> Result<bool, Value> {
        let station = self.get_station_status(station_id)?;
        for cm in json_get_list("crew", &station).unwrap() {
            if json_get_string("member_type", cm).unwrap() == "trader" {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn assign_trader_to_station(&self, station_id: u64, crew_id: u64) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/crew/assign/{crew_id}/trading"
        ))
    }

    pub fn compute_travel_cost(
        &self,
        origin: (u64, u64, u64),
        dest: (u64, u64, u64),
        speed: f64,
        fuelcons: f64,
        hullusage: f64,
        cargo_usage_perc: u8,
    ) -> ApiResult {
        self.post_with_body(
            "/calc/navigation",
            json!({
                "origin": origin,
                "dest": dest,
                "ship_stats": json!({
                    "speed": speed,
                    "fuel_consumption": fuelcons,
                    "hull_usage_rate": hullusage,
                }),
                "cargo_usage_perc": cargo_usage_perc,
            }),
        )
    }

    pub fn travel(&self, ship_id: u64, position: (u64, u64, u64), wait_end: bool) -> ApiResult {
        let (x, y, z) = position;
        let costs = self.post_with_body(
            format!("/ship/{ship_id}/navigate"),
            json!({
                "x": x,
                "y": y,
                "z": z,
            }),
        )?;
        if wait_end {
            let duration = json_get_float("duration", &costs).unwrap();
            std::thread::sleep(Duration::from_secs_f64(duration));
            self.wait_until_ship_idle(ship_id, Duration::from_secs(1))?;
        }
        Ok(costs)
    }

    pub fn stop_travel(&self, ship_id: u64) -> ApiResult {
        self.post(format!("/ship/{ship_id}/navigation/stop"))
    }

    pub fn wait_until_ship_idle(&self, ship_id: u64, time_sleep: Duration) -> Result<(), Value> {
        let init_ship = self.get_ship_status(ship_id)?;
        let mut idle_state = matches!(json_get_string("state", &init_ship), Some("Idle"));
        while !idle_state {
            std::thread::sleep(time_sleep);
            let ship = self.get_ship_status(ship_id)?;
            idle_state = matches!(json_get_string("state", &ship), Some("Idle"));
        }
        Ok(())
    }
    pub fn buy_hull_for_repair(
        &self,
        station_id: u64,
        ship_id: u64,
    ) -> Result<Option<Value>, Value> {
        let ship = self.get_ship_status(ship_id)?;
        let req = json_get_float("hull_decay", &ship).unwrap();

        // Pas besoin
        if req == 0.0 {
            return Ok(None);
        }

        let cargo = self.get_station_resources(station_id)?;
        let amnt_got = cargo.get("hull").cloned().unwrap_or(0.0);
        if amnt_got < req {
            let need = req - amnt_got;
            Ok(Some(self.buy_resource(station_id, "hull", need)?))
        } else {
            Ok(None)
        }
    }

    pub fn repair_ship(&self, station_id: u64, ship_id: u64) -> Result<Option<Value>, Value> {
        let ship = self.get_ship_status(ship_id)?;
        let req = json_get_float("hull_decay", &ship).unwrap();

        // Pas besoin
        if req == 0.0 {
            return Ok(None);
        }

        let cargo = self.get_station_resources(station_id)?;
        let amnt_got = cargo.get("hull").cloned().unwrap_or(0.0);

        if amnt_got > 0.0 {
            Ok(Some(
                self.post(format!("/station/{station_id}/repair/{ship_id}"))?,
            ))
        } else {
            Ok(None)
        }
    }

    pub fn buy_fuel_for_refuel(
        &self,
        station_id: u64,
        ship_id: u64,
    ) -> Result<Option<Value>, Value> {
        let ship = self.get_ship_status(ship_id)?;
        let current = json_get_float("fuel_tank", &ship).unwrap();
        let capacity = json_get_float("fuel_tank_capacity", &ship).unwrap();
        let req = capacity - current;

        // Pas besoin
        if req == 0.0 {
            return Ok(None);
        }

        let cargo = self.get_station_resources(station_id)?;
        let amnt_got = cargo.get("fuel").cloned().unwrap_or(0.0);
        if amnt_got < req {
            let need = req - amnt_got;
            Ok(Some(self.buy_resource(station_id, "fuel", need)?))
        } else {
            Ok(None)
        }
    }
    pub fn refuel_ship(&self, station_id: u64, ship_id: u64) -> Result<Option<Value>, Value> {
        let ship = self.get_ship_status(ship_id)?;
        let current = json_get_float("fuel_tank", &ship).unwrap();
        let capacity = json_get_float("fuel_tank_capacity", &ship).unwrap();
        let req = capacity - current;

        // Pas besoin
        if req == 0.0 {
            return Ok(None);
        }

        let cargo = self.get_station_resources(station_id)?;
        let amnt_got = cargo.get("fuel").cloned().unwrap_or(0.0);

        if amnt_got > 0.0 {
            Ok(Some(
                self.post(format!("/station/{station_id}/refuel/{ship_id}"))?,
            ))
        } else {
            Ok(None)
        }
    }
    pub fn scan_planets(&self, station_id: u64) -> Result<Vec<Value>, Value> {
        let station = self.get_station_status(station_id)?;
        let all_scanned = self.post(format!("/station/{station_id}/scan"))?;
        let mut all_planets = json_get_list("planets", &all_scanned).unwrap();
        all_planets.sort_by(|a, b| {
            let stapos = get_position(&station).unwrap();
            let posa = get_position(a).unwrap();
            let dista = get_dist(stapos, posa);
            let posb = get_position(b).unwrap();
            let distb = get_dist(stapos, posb);
            dista.partial_cmp(&distb).unwrap()
        });
        Ok(all_planets.iter().map(|v| (*v).clone()).collect())
    }
    pub fn start_extraction(&self, ship_id: u64) -> ApiResult {
        self.post(format!("/ship/{ship_id}/extraction/start"))
    }
    pub fn stop_extraction(&self, ship_id: u64) -> ApiResult {
        self.post(format!("/ship/{ship_id}/extraction/stop"))
    }
    pub fn unload(&self, station_id: u64, ship_id: u64, res: &str, amnt: f64) -> ApiResult {
        self.post(format!("/ship/{ship_id}/unload/{station_id}/{res}/{amnt}"))
    }
    pub fn unload_all(&self, station_id: u64, ship_id: u64) -> ApiResult {
        self.post(format!("/ship/{ship_id}/unload/{station_id}/all"))
    }
    pub fn return_station_and_unload_all(&self, station_id: u64, ship_id: u64) -> ApiResult {
        let ship = self.get_ship_status(ship_id)?;
        let station = self.get_station_status(station_id)?;

        let stapos = get_position(&station).unwrap();
        if get_position(&ship).unwrap() != stapos {
            self.travel(ship_id, stapos, true)?;
        }
        self.unload_all(station_id, ship_id)
    }
    pub fn get_station_resources(&self, station_id: u64) -> Result<HashMap<String, f64>, Value> {
        let station = self.get_station_status(station_id)?;
        let cargo = json_get_key("cargo.resources", &station).unwrap();
        let mut resources = HashMap::new();
        for (res, amnt) in cargo.as_object().unwrap() {
            resources.insert(res.clone(), amnt.as_f64().unwrap());
        }
        Ok(resources)
    }
    pub fn get_market_prices(&self) -> Result<HashMap<String, f64>, Value> {
        let prices = self.get("/market/prices")?;
        let mut resources = HashMap::new();
        for (res, amnt) in prices.as_object().unwrap() {
            resources.insert(res.clone(), amnt.as_f64().unwrap());
        }
        Ok(resources)
    }
    pub fn sell_resource(&self, station_id: u64, resource: &str, amnt: f64) -> ApiResult {
        self.post(format!("/market/{station_id}/sell/{resource}/{amnt}"))
    }
    pub fn buy_resource(&self, station_id: u64, resource: &str, amnt: f64) -> ApiResult {
        self.post(format!("/market/{station_id}/buy/{resource}/{amnt}"))
    }

    pub fn get_syslogs(&self) -> ApiResult {
        self.get("/syslogs")
    }

    #[allow(clippy::type_complexity)]
    pub fn get_resources_info(&self) -> Result<HashMap<String, (f64, f64, f64, u64)>, Value> {
        let got = self.get("/resources")?;
        let mut result = HashMap::new();
        for (res, val) in got.as_object().unwrap() {
            let volume = json_get_float("volume", val).unwrap();
            let base_price = json_get_float("base-price", val).unwrap();
            let difficulty = json_get_float("difficulty", val).unwrap_or(0.0);
            let minrank = json_get_uint("min-rank", val).unwrap_or(0);
            result.insert(res.clone(), (volume, base_price, difficulty, minrank));
        }
        Ok(result)
    }

    pub fn get_ship_wages_cost(&self, ship_id: u64) -> Result<f64, Value> {
        let got = self.get(format!("/ship/{ship_id}/wages"))?;
        Ok(json_get_float("wages", &got).unwrap())
    }

    pub fn upgrade_station_crew(&self, station_id: u64, crew_id: u64) -> ApiResult {
        self.post(format!("/station/{station_id}/crew/upgrade/{crew_id}"))
    }

    pub fn upgrade_station_cargo(&self, station_id: u64, ncargo: usize) -> ApiResult {
        self.post(format!("/station/{station_id}/shop/cargo/buy/{ncargo}"))
    }

    pub fn list_ship_crew_upgrades(
        &self,
        station_id: u64,
        ship_id: u64,
    ) -> Result<BTreeMap<u64, Value>, Value> {
        let mut result = BTreeMap::new();
        let res = self.get(format!("/station/{station_id}/crew/upgrade/ship/{ship_id}"))?;
        for (key, val) in res.as_object().unwrap() {
            let id = key.parse().unwrap();
            result.insert(id, val.clone());
        }
        Ok(result)
    }

    pub fn list_ship_module_upgrades(
        &self,
        station_id: u64,
        ship_id: u64,
    ) -> Result<BTreeMap<u64, Value>, Value> {
        let mut result = BTreeMap::new();
        let res = self.get(format!(
            "/station/{station_id}/shop/modules/{ship_id}/upgrade"
        ))?;
        for (key, val) in res.as_object().unwrap() {
            let id = key.parse().unwrap();
            result.insert(id, val.clone());
        }
        Ok(result)
    }

    pub fn list_ship_upgrades(
        &self,
        station_id: u64,
        ship_id: u64,
    ) -> Result<HashMap<String, Value>, Value> {
        let mut result = HashMap::new();
        let res = self.get(format!("/station/{station_id}/shipyard/upgrade/{ship_id}"))?;
        for (key, val) in res.as_object().unwrap() {
            result.insert(key.clone(), val.clone());
        }
        Ok(result)
    }

    pub fn upgrade_ship_crew(&self, station_id: u64, ship_id: u64, crew_id: u64) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/crew/upgrade/ship/{ship_id}/{crew_id}"
        ))
    }

    pub fn upgrade_ship_module(&self, station_id: u64, ship_id: u64, module_id: u64) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/shop/modules/{ship_id}/upgrade/{module_id}"
        ))
    }

    pub fn buy_ship_upgrade(&self, station_id: u64, ship_id: u64, upgtype: &str) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/shipyard/upgrade/{ship_id}/{upgtype}"
        ))
    }

    pub fn buy_ship_expansion(
        &self,
        station_id: u64,
        ship_id: u64,
        exptype: &str,
        n: f64,
    ) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/shipyard/upgrade/{ship_id}/expand/{exptype}/{n}"
        ))
    }

    pub fn compute_mining_rate(
        &self,
        modtype: &str,
        modrank: u8,
        oprank: u8,
        planet: (u64, u64, u64),
        power_attr: Option<&[(String, u8)]>,
    ) -> Result<HashMap<String, f64>, Value> {
        let mut pa = HashMap::new();
        if let Some(power_attr) = power_attr {
            for (key, val) in power_attr {
                pa.insert(key, val);
            }
        }
        let data = self.post_with_body(
            "/calc/mining_rate",
            json!({
                "module_type": modtype,
                "module_rank": modrank,
                "operator_rank": oprank,
                "planet_coord": planet,
                "power_attribution": pa,
            }),
        )?;
        let mut result = HashMap::new();
        for (res, rate) in data.as_object().unwrap() {
            result.insert(res.clone(), rate.as_f64().unwrap());
        }
        Ok(result)
    }

    pub fn compute_ship_mining_rate(
        &self,
        ship_id: u64,
        planet: (u64, u64, u64),
        power_attr: Option<&[(String, u8)]>,
    ) -> Result<HashMap<String, f64>, Value> {
        let mut pa = HashMap::new();
        if let Some(power_attr) = power_attr {
            for (key, val) in power_attr {
                pa.insert(key, val);
            }
        }
        let data = self.post_with_body(
            format!("/calc/mining_rate/{ship_id}"),
            json!({
                "planet_coord": planet,
                "power_attribution": pa,
            }),
        )?;
        let mut result = HashMap::new();
        for (res, rate) in data.as_object().unwrap() {
            result.insert(res.clone(), rate.as_f64().unwrap());
        }
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn compute_upgrade_ship(
        &self,
        start_reactor: u8,
        end_reactor: u8,
        start_shield: u8,
        end_shield: u8,
        start_pilot: u8,
        end_pilot: u8,
        start_cargo_capacity: f64,
        end_cargo_capacity: f64,
    ) -> ApiResult {
        self.post_with_body(
            "/calc/upgrade/ship",
            json!({
                "start_pilot_rank": start_pilot,
                "end_pilot_rank": end_pilot,
                "start_reactor_rank": start_reactor,
                "end_reactor_rank": end_reactor,
                "start_shield_rank": start_shield,
                "end_shield_rank": end_shield,
                "start_cargo_capacity": start_cargo_capacity,
                "end_cargo_capacity": end_cargo_capacity,
            }),
        )
    }

    pub fn compute_upgrade_module(
        &self,
        modtype: &str,
        start_mod: u8,
        end_mod: u8,
        start_op: u8,
        end_op: u8,
    ) -> ApiResult {
        self.post_with_body(
            "/calc/upgrade/module",
            json!({
                "module_type": modtype,
                "module_start_rank": start_mod,
                "module_end_rank": end_mod,
                "crew_start_rank": start_op,
                "crew_end_rank": end_op,
            }),
        )
    }

    pub fn compute_upgrade_trader(&self, start_rank: u8, end_rank: u8) -> ApiResult {
        self.post_with_body(
            "/calc/upgrade/trader",
            json!({
                "trader_start_rank": start_rank,
                "trader_end_rank": end_rank,
            }),
        )
    }

    pub fn game_version(&self) -> Result<String, Value> {
        let res = self.get("/version")?;
        Ok(json_get_string("version", &res).unwrap().to_string())
    }

    pub fn get_game_stats(&self) -> ApiResult {
        self.get("/gamestats")
    }

    pub fn get_fee_rate(&self, station_id: u64) -> Result<f64, Value> {
        let res = self.get(format!("/market/{station_id}/fee_rate"))?;
        let fr = json_get_float("fee_rate", &res).unwrap();
        Ok(fr)
    }

    pub fn list_industry_to_buy(&self, station_id: u64) -> ApiResult {
        self.get(format!("/station/{station_id}/industry/buy"))
    }

    pub fn buy_industry(&self, station_id: u64, name: &str) -> ApiResult {
        self.post(format!("/station/{station_id}/industry/buy/{name}"))
    }

    pub fn get_industry_production(&self, station_id: u64, industry_id: u64) -> ApiResult {
        self.get(format!(
            "/station/{station_id}/industry/production/{industry_id}"
        ))
    }

    pub fn assign_crew_to_industry(
        &self,
        station_id: u64,
        crew_id: u64,
        industry_id: u64,
    ) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/crew/assign/{crew_id}/industry/{industry_id}"
        ))
    }

    pub fn start_industry(&self, station_id: u64, industry_id: u64) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/industry/start/{industry_id}"
        ))
    }

    pub fn stop_industry(&self, station_id: u64, industry_id: u64) -> ApiResult {
        self.post(format!("/station/{station_id}/industry/stop/{industry_id}"))
    }

    pub fn upgrade_industry(&self, station_id: u64, industry_id: u64) -> ApiResult {
        self.post(format!(
            "/station/{station_id}/industry/upgrade/{industry_id}"
        ))
    }

    pub fn get_extraction_power_attribution(
        &self,
        ship_id: u64,
    ) -> Result<HashMap<String, u8>, Value> {
        let res = self.get(format!("/ship/{ship_id}/extraction/power_attribution"))?;
        let hmap = json_get_dict("power_attribution", &res).unwrap();
        let mut result = HashMap::new();
        for (res, val) in hmap.into_iter() {
            result.insert(res.clone(), u8::try_from(val.as_u64().unwrap()).unwrap());
        }
        Ok(result)
    }

    pub fn set_extraction_power_attribution(
        &self,
        ship_id: u64,
        res: String,
        pow: u8,
    ) -> ApiResult {
        self.post(format!(
            "/ship/{ship_id}/extraction/power_attribution/{res}/{pow}"
        ))
    }

    pub fn get_all_wages(&self) -> ApiResult {
        self.get("/player/wages")
    }

    pub fn get_station_wages(&self, sta: u64) -> ApiResult {
        self.get(format!("/station/{sta}/wages"))
    }

    pub fn unassign_crew(&self, sta: u64, cid: u64) -> ApiResult {
        self.post(format!("/station/{sta}/crew/unassign/{cid}"))
    }

    pub fn get_game_config(&self) -> ApiResult {
        self.get("/configuration")
    }

    pub fn compute_ship_wages(
        &self,
        pilot_rank: Option<u8>,
        opranks: Vec<u8>,
    ) -> Result<f64, Value> {
        let got = if let Some(pilotrank) = pilot_rank {
            self.post_with_body(
                "/calc/wages/ship",
                json!({
                    "pilot_rank": pilotrank,
                    "operator_ranks": opranks,
                }),
            )
        } else {
            self.post_with_body(
                "/calc/wages/ship",
                json!({
                    "operator_ranks": opranks,
                }),
            )
        };
        got.map(|v| json_get_float("total_wages", &v).unwrap())
    }

    pub fn get_player_history(&self) -> Result<Vec<Value>, Value> {
        let mut got = self.get("/player/history")?;
        let got = got.as_object_mut().unwrap();
        let Some(serde_json::Value::Array(data)) = got.remove("history") else {
            unreachable!();
        };
        Ok(data)
    }

    pub fn reassign_operator(&self, ship_id: u64, operator_id: u64, module_id: u64) -> ApiResult {
        self.post(format!(
            "/ship/{ship_id}/reassign/{operator_id}/{module_id}"
        ))
    }
}
