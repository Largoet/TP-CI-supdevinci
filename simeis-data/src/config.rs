use std::{path::Path, sync::OnceLock};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::errors::Errcode;

const INIT_MODULE_PRICE: f64 = 4000.0;
const INIT_MONEY_AFTER_FIRST_SETUP: f64 = 10000.0;

static CONFIG: OnceLock<SimeisConfig> = OnceLock::new();

pub fn get_config<'a>() -> &'a SimeisConfig {
    CONFIG.get().unwrap()
}

pub fn init_default_config() {
    let cfg = SimeisConfig::default();
    let _ = CONFIG.set(cfg);
}

pub fn init_config<'a>(fpath: &Path) -> Result<&'a SimeisConfig, Errcode> {
    let cfg = if !fpath.is_file() {
        println!("Config file is missing, generating one from default configuration...");
        let cfg = SimeisConfig::default();
        let cfg_str = serde_json::to_string_pretty(&cfg).map_err(Errcode::DefaultConfigToJson)?;
        std::fs::write(fpath, cfg_str).map_err(Errcode::WriteDefaultConfig)?;
        cfg
    } else {
        let s = std::fs::read_to_string(fpath).map_err(Errcode::ReadConfigFile)?;
        let cfg: SimeisConfig__deser = serde_json::from_str(&s).map_err(Errcode::LoadConfigJson)?;
        SimeisConfig::from(cfg)
    };

    CONFIG.set(cfg).unwrap();
    Ok(CONFIG.get().unwrap())
}

fn get_val<T: DeserializeOwned>(val: Option<T>, envn: &'static str, defval: T) -> T {
    if !envn.is_empty() {
        if let Ok(v) = std::env::var(envn) {
            return serde_json::from_str(&v).expect("Unable to load env var value");
        }
    }
    if let Some(v) = val {
        v
    } else {
        defval
    }
}

macro_rules! config_fields {
    ($struct_name:ident; $($name:ident: $t:ty => $envn:expr => $defval:expr $(,)*),*) => {
        paste::paste!{
        #[derive(Serialize, Debug)]
        pub struct [< $struct_name >] {
            $( pub $name: $t, )*
        }

        #[derive(Deserialize, Default)]
        #[allow(non_camel_case_types)]
        pub struct [< $struct_name __deser >] {
            $(
                #[serde(default)]
                $name: Option<$t>,
            )*
        }

        impl [< $struct_name __deser >] {
            pub fn resolve(self) -> $struct_name {
                $struct_name {
                $(
                    $name: get_val(self.$name, $envn, $defval),
                )*
                }
            }
        }
        }
    };
}
macro_rules! main_config {
    ($name:ident; $($n:ident: $cfgty:ty $(,)*),*) => {
        paste::paste!{
        #[derive(Deserialize, Default)]
        #[allow(non_camel_case_types)]
        pub struct [< $name __deser >] {
            $(
                #[serde(default)]
                $n: [< $cfgty __deser >],
            )*
        }

        impl From<[< $name __deser >]> for $name {
            fn from(value: [< $name __deser >]) -> $name {
                $name {
                    $(
                        $n: value.$n.resolve(),
                    )*
                }
            }
        }
        }

        #[derive(Serialize, Debug)]
        pub struct $name {
            $(
                pub $n: $cfgty,
            )*
        }

        paste::paste!{
        impl Default for $name {
            fn default() -> $name {
                let d = [< $name __deser >]::default();
                $name::from(d)
            }
        }
        }
    };
}

config_fields!(SimeisServerConfig;
    port: u16 => "SIMEIS_PORT" => 8080,
    workers: usize => "SIMEIS_WORKERS" => 64,
    bind_ip: String => "" => "0.0.0.0".to_string(),
    logger_env_name: String => "" => "SIMEIS_LOG".to_string(),
    token_file: String => "SIMEIS_TOKEN_FILE" => ".simeis-admin-token".to_string(),
);

config_fields!(SimeisGameConfig;
    shards: usize => "SIMEIS_LOCKS_SHARDS" => 5,
    player_init_money: f64 => "SIMEIS_PLAYER_INIT_MONEY" => 86000.0 + INIT_MONEY_AFTER_FIRST_SETUP + INIT_MODULE_PRICE,
    sector_size: u32 => "" => 2000,
    planets_per_sector: usize => "" => 6,
    station_fplanet_dist: f64 => "" => 220.0,
);

config_fields!(SimeisUpgradeConfig;
    industry_unitupg_powf_div: f64 => "" => 75.0,
    module_upg_powf_div: f64 => "" => 75.0,
);

config_fields!(SimeisShipConfig;
    fuel_tank_cap_price: f64 => "" => 50.0,
    fuel_consumption_mult: f64 => "" => 9.0,
    cargo_cap_price: f64 => "" => 40.0,
    hull_resis_price: f64 => "" => 8.0,
    reactor_rank_price: f64 => "" => 4000.0,
    shield_price: f64 => "" => 1000.0,
    default_pow_attr: u8 => "" => 128,

    module_base_price: f64 => "" => INIT_MODULE_PRICE,
    module_base_extraction_mult: f64 => "" => 45.0,
    module_exrate_diff_fact: f64 => "" => 1.3,
    module_exrate_fact: f64 => "" => 0.6,
    module_rank_weight: f64 => "" => 2.5,

    hull_usage_rate: f64 => "" => 30.0 / 1000.0,
    reactor_speed_rate: f64 => "" => 55.0,
    shield_rank_log: f64 => "" => 3.0,
    pilot_fuel_share: u8 => "" => 9,
    pilot_rank_speed_powf: f64 => "" => 1.0,
);
config_fields!(SimeisStationConfig;
    cargo_base_price: f64 => "" => 80.0,
    cargo_price_incdiv: f64 => "" => 100.0,
    init_cargo: f64 => "" => 10000.0,
);
config_fields!(SimeisCrewConfig;
    wage_rank_powf: f64 => "" => 2.2,
    wage_next_rank_powf: f64 => "" => 0.9,
    wage_next_rank_mult: f64 => "" => 1100.0,

    base_op_wage: f64 => "" => 0.85,
    pilot_wage_mult: f64 => "" => 3.7,
    trader_wage_mult: f64 => "" => 3.5,
    solider_wage_mult: f64 => "" => 2.5,
);

config_fields!(SimeisMarketConfig;
    max_avg_ampl: f64 => "" => 2.0 / 100.0,
    std_div: f64 => "" => 1.5,
    change_sec: f64 => "" => 4.0,
    base_fee_rate: f64 => "" => 28.0 / 100.0,
    fee_rate_dev_powf: f64 => "" => 0.92,
    upd_price_proba: f64 => "" => 0.5,

    influence_max: f64 => "" => 0.35,

    buy_influence_param_a: f64 => "" => 0.012016306907210268,
    buy_influence_param_b: f64 => "" => 0.6237236148551031,
    buy_influence_param_c: f64 => "" => 1.5387935484111805,
    buy_influence_param_d: f64 => "" => 0.083936408655154,
    buy_influence_param_e: f64 => "" => 0.023142091704532564,
    buy_influence_param_f: f64 => "" => 0.0341811445383504,

    sell_influence_param_a: f64 => "" => 0.02578237202951275,
    sell_influence_param_b: f64 => "" => 0.5554258819004403,
    sell_influence_param_c: f64 => "" => 1.9919988458913422,
    sell_influence_param_d: f64 => "" => 0.0018077576687068689,
    sell_influence_param_e: f64 => "" => 0.010908391555243003,
    sell_influence_param_f: f64 => "" => 0.002494236546541542,
);

config_fields!(SimeisIndustryConfig;
    simple_bprice_mult: f64 => "" => 1.08,
    simple_unit_price: f64 => "" => 8000.0,
    advanced_bprice_mult: f64 => "" => 1.75,
    advanced_unit_price: f64 => "" => 18000.0,

    simple_industry_tier1_resources_req: f64 => "" => 1.5,
    simple_res2rq: f64 => "" => 0.2,
    simple_res3rq: f64 => "" => 1.25,
    simple_res4rq: f64 => "" => 0.4,

    advanced_industry_tier1_resources_req: f64 => "" => 7.5,
    advanced_res2rq: f64 => "" => 0.4,
    advanced_res3rq: f64 => "" => 0.2,
);
config_fields!(SimeisResourcesConfig;
    tier1_price: f64 => "" => 7.5,
    tier2_price_mult: f64 => "" => 2.0,
    tier3_price_mult: f64 => "" => 5.3,
    tier4_price_mult: f64 => "" => 8.2,

    tier1_volume: f64 => "" => 2.3,
    tier2_volume: f64 => "" => 1.7,
    tier3_volume: f64 => "" => 1.15,
    tier4_volume: f64 => "" => 0.8,

    fuel_price: f64 => "" => 6.0,
    hull_price: f64 => "" => 0.35,
    fuel_volume: f64 => "" => 5.0,
    hull_volume: f64 => "" => 0.65,

    tier1_difficulty: f64 => "" => 1.9,
    tier2_difficulty_mult: f64 => "" => 2.1,
    tier3_difficulty_mult: f64 => "" => 5.0,
    tier4_difficulty_mult: f64 => "" => 8.0,

    tier1_min_rank: u8 => "" => 0,
    tier2_min_rank: u8 => "" => 2,
    tier3_min_rank: u8 => "" => 4,
    tier4_min_rank: u8 => "" => 6,
);
main_config!(SimeisConfig;
    server: SimeisServerConfig,
    game: SimeisGameConfig,
    resources: SimeisResourcesConfig,
    industry: SimeisIndustryConfig,
    upgrade: SimeisUpgradeConfig,
    station: SimeisStationConfig,
    ship: SimeisShipConfig,
    crew: SimeisCrewConfig,
    market: SimeisMarketConfig,
);

impl SimeisMarketConfig {
    #[inline]
    pub fn influence_buy(&self, sold_val: f64, price: f64) -> f64 {
        let amnt = (sold_val / 1000.0).max(1e-6);
        let base = self.buy_influence_param_a * (amnt.powf(self.buy_influence_param_b))
            / (price.powf(self.buy_influence_param_c));
        let mult = 1.0
            + (self.buy_influence_param_d * (amnt.powf(self.buy_influence_param_e))
                / (price.powf(self.buy_influence_param_f)));
        let infl = base * mult;
        debug_assert!(infl >= 0.0);
        1.0 + infl.min(self.influence_max)
    }

    #[inline]
    pub fn influence_sell(&self, sold_val: f64, price: f64) -> f64 {
        let amnt = (sold_val / 1000.0).max(1e-6);
        let base = self.sell_influence_param_a
            * amnt.powf(self.sell_influence_param_b)
            * price.powf(self.sell_influence_param_c);
        let mult = 1.0
            + (self.sell_influence_param_d
                * amnt.powf(self.sell_influence_param_e)
                * price.powf(self.sell_influence_param_f));
        let infl = base * mult;
        debug_assert!(infl >= 0.0);
        1.0 - infl.min(self.influence_max)
    }
}

impl SimeisConfig {
    pub fn validation_tests(&self) {
        // Market influence
        let inflb = self.market.influence_buy(100.0, 1.0);
        println!("Influence BUY for 100 at 100%: {inflb}");
        assert!(inflb < 1.01);
        let infls = self.market.influence_sell(100.0, 1.0);
        println!("Influence SELL for 100 at 100%: {infls}");
        assert!(infls > 0.99);
    }
}
