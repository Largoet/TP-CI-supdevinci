use serde::{Deserialize, Serialize};

use super::resources::Resource;
use crate::config::get_config;
use crate::crew::{Crew, CrewId, CrewMemberType};
use crate::define_type_enum;
use crate::galaxy::planet::Planet;

pub type ShipModuleId = u16;

define_type_enum!(ShipModuleType {
    Miner,
    GasSucker,
    Pump,
});

impl ShipModuleType {
    pub fn new_module(self) -> ShipModule {
        ShipModule {
            operator: None,
            modtype: self,
            rank: 1,
        }
    }

    #[inline]
    pub fn get_price_buy(&self) -> f64 {
        let cfg = get_config();
        match self {
            ShipModuleType::Miner | ShipModuleType::Pump | ShipModuleType::GasSucker => {
                cfg.ship.module_base_price
            }
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShipModule {
    pub operator: Option<CrewId>,
    pub modtype: ShipModuleType,
    pub rank: u8,
}

impl ShipModule {
    pub fn new(modtype: ShipModuleType, rank: u8) -> ShipModule {
        ShipModule {
            operator: None,
            modtype,
            rank,
        }
    }

    #[inline]
    pub fn price_next_rank(&self) -> f64 {
        let cfg = get_config();
        let num = cfg.upgrade.module_upg_powf_div - 1.0 + (self.rank as f64);
        self.modtype
            .get_price_buy()
            .powf(num / cfg.upgrade.module_upg_powf_div)
    }

    // Returns
    pub fn need(&self, ctype: &CrewMemberType) -> bool {
        match self.modtype {
            ShipModuleType::Miner | ShipModuleType::Pump | ShipModuleType::GasSucker => {
                ctype == &CrewMemberType::Operator && self.operator.is_none()
            }
        }
    }

    pub fn can_extract(&self, crew: &Crew, planet: &Planet) -> Vec<(Resource, f64)> {
        let Some(ref opid) = self.operator else {
            return vec![];
        };

        let cm = crew.0.get(opid).unwrap();
        planet
            .resources_density
            .iter()
            .filter(|(r, _)| r.extractable(&self.modtype, cm.rank))
            .map(|(r, density)| (*r, calc_extraction_rate(r, self.rank, cm.rank, *density)))
            .collect()
    }
}

#[inline]
pub fn calc_extraction_rate(resource: &Resource, modrank: u8, oprank: u8, density: f64) -> f64 {
    let cfg = get_config();
    let rank = ((oprank - resource.min_rank()) as f64)
        * (modrank as f64).powf(cfg.ship.module_rank_weight);
    let difficulty = resource
        .extraction_difficulty()
        .powf(cfg.ship.module_exrate_diff_fact);
    cfg.ship.module_base_extraction_mult
        * density
        * (rank / difficulty).powf(cfg.ship.module_exrate_fact)
}
