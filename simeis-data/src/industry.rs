use std::collections::BTreeMap;

use rand::RngExt;
use serde::Serialize;

use crate::config::{get_config, SimeisIndustryConfig};
use crate::crew::{CrewId, CrewMember, CrewMemberType};
use crate::define_type_enum;
use crate::ship::resources::Resource;

pub type IndustryUnitId = u32;

// Because all resources of the same level have the same base price
// The resource cost (in credits) should be the same whatever the unit is
// As long as it's the same class (simple / advanced)
#[inline]
pub fn get_simple_industry_resources_cost(cfg: &SimeisIndustryConfig) -> f64 {
    let sbase_req = cfg.simple_industry_tier1_resources_req;
    (sbase_req * Resource::Hydrogen.base_price())             // Gas 1
        + (sbase_req * cfg.simple_res2rq * Resource::Oxygen.base_price())   // Gas 2
        + (sbase_req * cfg.simple_res3rq * Resource::Iron.base_price())    // Solid 1
        + (sbase_req * cfg.simple_res4rq * Resource::Water.base_price()) // Liquid 1
}

#[inline]
pub fn get_advanced_industry_resources_cost(cfg: &SimeisIndustryConfig) -> f64 {
    let abase_req = cfg.advanced_industry_tier1_resources_req;
    (abase_req * Resource::Iron.base_price())                     // Solid 1
        + (abase_req * cfg.advanced_res2rq * Resource::SulfuricAcid.base_price()) // Liquid 4
        + (abase_req * cfg.advanced_res3rq * Resource::Helium.base_price()) // Gas 3
}

// TODO (#63)     Must update this:   The fuel and hull base prices are different now
#[inline]
pub fn get_sbase_produce_base(cfg: &SimeisIndustryConfig) -> f64 {
    let scost = get_simple_industry_resources_cost(cfg);
    scost / (cfg.simple_bprice_mult * Resource::Fuel.base_price())
}

// TODO (#63)    Must update this:   The fuel and hull base prices are different now
#[inline]
pub fn get_abase_produce_base(cfg: &SimeisIndustryConfig) -> f64 {
    let acost = get_advanced_industry_resources_cost(cfg);
    acost / (cfg.advanced_bprice_mult * Resource::Fuel.base_price())
}

define_type_enum!(IndustryUnitType {
    SimpleFuelRefinery,
    AdvancedFuelRefinery,

    SimpleHullFoundry,
    AdvancedHullFoundry,
});

impl IndustryUnitType {
    pub fn new_unit(self) -> IndustryUnit {
        let unitid = rand::rng().random();
        IndustryUnit {
            id: unitid,
            operator: None,
            unittype: self,
            rank: 1,
            started: false,
            resources_required: vec![],
            resources_created: vec![],
        }
    }

    #[inline]
    // TODO (#9) Move price based on inflation rate of the station
    pub fn get_price_buy(&self) -> f64 {
        let cfg = get_config();
        match self {
            IndustryUnitType::SimpleHullFoundry | IndustryUnitType::SimpleFuelRefinery => {
                cfg.industry.simple_unit_price
            }
            IndustryUnitType::AdvancedHullFoundry | IndustryUnitType::AdvancedFuelRefinery => {
                cfg.industry.advanced_unit_price
            }
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct IndustryUnit {
    pub id: IndustryUnitId,
    pub unittype: IndustryUnitType,
    pub rank: u8,
    pub started: bool,

    pub operator: Option<CrewId>,
    resources_required: Vec<(Resource, f64)>,
    resources_created: Vec<(Resource, f64)>,
}

impl IndustryUnit {
    #[inline]
    pub fn price_next_rank(&self) -> f64 {
        let cfg = get_config();
        let num = cfg.upgrade.industry_unitupg_powf_div - 1.0 + (self.rank as f64);
        self.unittype
            .get_price_buy()
            .powf(num / cfg.upgrade.industry_unitupg_powf_div)
    }

    #[inline]
    pub fn need_crew_member(&self, ctype: &CrewMemberType) -> bool {
        ctype == &CrewMemberType::Operator && self.operator.is_none()
    }

    #[inline]
    pub fn assign_operator(&mut self, opid: CrewId, op: &CrewMember) {
        self.operator = Some(opid);
        self.new_op_rank(op.rank);
    }

    #[inline]
    pub fn new_op_rank(&mut self, rank: u8) {
        self.resources_required = self.input(rank);
        self.resources_created = self.output(rank);
    }

    #[inline]
    pub fn input(&self, oprank: u8) -> Vec<(Resource, f64)> {
        debug_assert_ne!(oprank, 0);
        let div = 1.0 / (std::f64::consts::E + (oprank as f64) - 1.0).ln();
        let cfg = get_config();
        let sbase = cfg.industry.simple_industry_tier1_resources_req * (self.rank as f64);
        let abase = cfg.industry.advanced_industry_tier1_resources_req * (self.rank as f64);
        match self.unittype {
            IndustryUnitType::SimpleFuelRefinery => {
                vec![
                    (Resource::Hydrogen, sbase),                            // Gas 1
                    (Resource::Oxygen, sbase * cfg.industry.simple_res2rq), // Gas 2
                    (Resource::Iron, sbase * cfg.industry.simple_res3rq),   // Solid 1
                    (Resource::Water, sbase * cfg.industry.simple_res4rq),  // Liquid 1
                ]
            }
            IndustryUnitType::SimpleHullFoundry => {
                vec![
                    (Resource::Iron, sbase),                                  // Solid 1
                    (Resource::Copper, sbase * cfg.industry.simple_res2rq),   // Solid 2
                    (Resource::Hydrogen, sbase * cfg.industry.simple_res3rq), // Gas 1
                    (Resource::Water, sbase * cfg.industry.simple_res4rq),    // Liquid 1
                ]
            }
            IndustryUnitType::AdvancedFuelRefinery => {
                vec![
                    (Resource::Iron, abase),                                        // Solid 1
                    (Resource::SulfuricAcid, abase * cfg.industry.advanced_res2rq), // Liquid 3
                    (Resource::Helium, abase * cfg.industry.advanced_res3rq),       // Gas 3
                ]
            }
            IndustryUnitType::AdvancedHullFoundry => {
                vec![
                    (Resource::Hydrogen, abase),                                // Gas 1
                    (Resource::Titanium, abase * cfg.industry.advanced_res2rq), // Solid 3
                    (Resource::Brine, abase * cfg.industry.advanced_res3rq),    // Liquid 3
                ]
            }
        }
        .into_iter()
        .map(|(res, amnt)| {
            let amnt: f64 = amnt;
            let new_amnt = amnt.powf(div);
            (res, new_amnt)
        })
        .collect()
    }

    #[inline]
    pub fn output(&self, oprank: u8) -> Vec<(Resource, f64)> {
        debug_assert_ne!(oprank, 0);
        let pown = (oprank as f64).ln();
        let cfg = &get_config().industry;

        match self.unittype {
            IndustryUnitType::SimpleFuelRefinery => vec![(
                Resource::Fuel,
                get_sbase_produce_base(cfg) * (self.rank as f64),
            )],
            IndustryUnitType::SimpleHullFoundry => vec![(
                Resource::Hull,
                get_sbase_produce_base(cfg) * (self.rank as f64),
            )],
            IndustryUnitType::AdvancedFuelRefinery => vec![(
                Resource::Fuel,
                get_abase_produce_base(cfg) * (self.rank as f64),
            )],
            IndustryUnitType::AdvancedHullFoundry => vec![(
                Resource::Hull,
                get_abase_produce_base(cfg) * (self.rank as f64),
            )],
        }
        .into_iter()
        .map(|(res, amnt)| {
            let amnt: f64 = amnt;
            (res, amnt.powf(pown))
        })
        .collect()
    }

    pub fn can_work(&self, tdelta: &f64, resources: &BTreeMap<Resource, f64>) -> Option<f64> {
        if !self.started {
            return None;
        }
        self.operator?;
        let mut max_ratio: f64 = 1.0;
        for (res, amnt) in self.resources_required.iter() {
            {
                let incargo = resources.get(res)?;
                let max = amnt * tdelta;
                max_ratio = max_ratio.min((incargo / max).min(1.0));
            }
        }
        log::warn!("Max ratio: {max_ratio:?}");
        Some(max_ratio)
    }

    pub fn work(&self, tdelta: f64, resources: &mut BTreeMap<Resource, f64>) {
        debug_assert!(self.started);
        debug_assert!(self.operator.is_some());
        for (res, amnt) in self.resources_required.iter() {
            let n = resources.get_mut(res).unwrap();
            *n -= amnt * tdelta;
            debug_assert!(*n >= 0.0);
            log::warn!("Used {} of {res:?}, got {n} left", amnt * tdelta);
        }

        for (res, amnt) in self.resources_created.iter() {
            if !resources.contains_key(res) {
                log::warn!("Doesn't have {res} yet in cargo: init to 0.0");
                resources.insert(*res, 0.0);
            }

            let n = resources.get_mut(res).unwrap();
            *n += amnt * tdelta;
            log::warn!("Created {} of {res:?}, got {n} now", amnt * tdelta);
        }
    }
}
