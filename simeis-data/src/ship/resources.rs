use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::config::get_config;
use crate::define_type_enum;
use crate::galaxy::planet::Planet;
use crate::ship::module::ShipModuleType;
use crate::ship::{cargo::ShipCargo, Ship};

define_type_enum!(Resource {
    // Solid
    Iron,
    Copper,
    Titanium,
    Uranium,

    // Liquid
    Water,
    Ammonia,
    SulfuricAcid,
    Brine,

    // Gaseous
    Hydrogen,
    Oxygen,
    Helium,
    Ozone,

    // Crafted
    Fuel,
    Hull,
});

impl Resource {
    #[inline]
    pub fn scored(&self) -> bool {
        matches!(self, Resource::Fuel | Resource::Hull)
    }

    #[inline]
    pub fn base_price(&self) -> f64 {
        let cfg = get_config();
        let base = cfg.resources.tier1_price;
        match self {
            Resource::Iron | Resource::Hydrogen | Resource::Water => base,
            Resource::Copper | Resource::Oxygen | Resource::Ammonia => {
                cfg.resources.tier2_price_mult * base
            }
            Resource::Titanium | Resource::Helium | Resource::SulfuricAcid => {
                cfg.resources.tier3_price_mult * base
            }
            Resource::Uranium | Resource::Ozone | Resource::Brine => {
                cfg.resources.tier4_price_mult * base
            }
            Resource::Fuel => cfg.resources.fuel_price,
            Resource::Hull => cfg.resources.hull_price,
        }
    }

    #[inline]
    pub fn volume(&self) -> f64 {
        let cfg = get_config();
        match self {
            Resource::Iron | Resource::Hydrogen | Resource::Water => cfg.resources.tier1_volume,
            Resource::Copper | Resource::Oxygen | Resource::Ammonia => cfg.resources.tier2_volume,
            Resource::Titanium | Resource::Helium | Resource::SulfuricAcid => {
                cfg.resources.tier3_volume
            }
            Resource::Uranium | Resource::Ozone | Resource::Brine => cfg.resources.tier4_volume,
            Resource::Fuel => cfg.resources.fuel_volume,
            Resource::Hull => cfg.resources.hull_volume,
        }
    }

    #[allow(unused_mut)]
    #[inline]
    pub fn extraction_difficulty(&self) -> f64 {
        let cfg = get_config();
        let mut base = cfg.resources.tier1_difficulty;

        #[cfg(feature = "extraspeed")]
        {
            base /= 1000.0;
        }
        match self {
            Resource::Iron | Resource::Hydrogen | Resource::Water => base,
            Resource::Copper | Resource::Oxygen | Resource::Ammonia => {
                cfg.resources.tier2_difficulty_mult * base
            }
            Resource::Titanium | Resource::Helium | Resource::SulfuricAcid => {
                cfg.resources.tier3_difficulty_mult * base
            }
            Resource::Uranium | Resource::Ozone | Resource::Brine => {
                cfg.resources.tier4_difficulty_mult * base
            }

            // All the things that are only crafted
            _ => unreachable!("Extraction difficulty on crafted resources"),
        }
    }

    #[inline]
    pub fn min_rank(&self) -> u8 {
        let cfg = get_config();
        match self {
            Resource::Iron | Resource::Hydrogen | Resource::Water => cfg.resources.tier1_min_rank,
            Resource::Copper | Resource::Oxygen | Resource::Ammonia => cfg.resources.tier2_min_rank,
            Resource::Titanium | Resource::Helium | Resource::SulfuricAcid => {
                cfg.resources.tier3_min_rank
            }
            Resource::Uranium | Resource::Ozone | Resource::Brine => cfg.resources.tier4_min_rank,
            Resource::Fuel | Resource::Hull => 0,
        }
    }

    #[inline]
    pub fn extractable(&self, modtype: &ShipModuleType, oprank: u8) -> bool {
        match modtype {
            ShipModuleType::Miner => self.mineable(oprank),
            ShipModuleType::GasSucker => self.suckable(oprank),
            ShipModuleType::Pump => self.pumpable(oprank),
        }
    }

    #[inline]
    pub fn mineable(&self, rank: u8) -> bool {
        match self {
            Resource::Iron | Resource::Copper | Resource::Titanium | Resource::Uranium => {
                rank > self.min_rank()
            }
            _ => false,
        }
    }

    #[inline]
    pub fn suckable(&self, rank: u8) -> bool {
        match self {
            Resource::Hydrogen | Resource::Oxygen | Resource::Helium | Resource::Ozone => {
                rank > self.min_rank()
            }
            _ => false,
        }
    }

    #[inline]
    pub fn pumpable(&self, rank: u8) -> bool {
        match self {
            Resource::Water | Resource::Ammonia | Resource::SulfuricAcid | Resource::Brine => {
                rank > self.min_rank()
            }
            _ => false,
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct ExtractionInfo {
    pub mining_rate: BTreeMap<Resource, f64>,
    pub time_fill_cargo: f64,
}

impl ExtractionInfo {
    pub fn create(ship: &Ship, planet: &Planet) -> Self {
        let mut extraction = BTreeMap::new();
        let mut cap_per_sec = 0.0;
        for smod in ship.modules.values() {
            for (res, rate) in smod.can_extract(&ship.crew, planet) {
                cap_per_sec += rate * res.volume();
                if let Some(rrate) = extraction.get_mut(&res) {
                    *rrate += rate;
                } else {
                    extraction.insert(res, rate);
                }
            }
        }
        apply_power_attribution(&ship.power_attribution, &mut extraction);
        ExtractionInfo {
            mining_rate: extraction,
            time_fill_cargo: ship.cargo.capacity / cap_per_sec,
        }
    }

    pub fn update_cargo(&self, cargo: &mut ShipCargo, tdelta: f64) -> bool {
        for (res, rate) in self.mining_rate.iter() {
            cargo.add_resource(res, *rate * tdelta);
        }
        cargo.is_full()
    }
}

// TODO (#13) Parametric test, whatever the resource distr, should never have nan or infinite vals
pub fn apply_power_attribution(attrs: &BTreeMap<Resource, u8>, r: &mut BTreeMap<Resource, f64>) {
    let cfg = get_config();
    let powtot: u16 = r
        .keys()
        .map(|r| *attrs.get(r).unwrap_or(&cfg.ship.default_pow_attr) as u16)
        .sum();
    r.iter_mut().for_each(|(res, rate)| {
        let power = attrs.get(res).unwrap_or(&cfg.ship.default_pow_attr);
        if power == &0 {
            *rate = 0.0;
        } else {
            let power_f = (*power as f64) / (powtot as f64);
            *rate *= power_f;
        }
    });
}
