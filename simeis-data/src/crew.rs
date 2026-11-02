use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{config::get_config, define_type_enum};

pub type CrewId = u32;

define_type_enum!(CrewMemberType {
    Pilot,
    Operator,
    Trader,
    Soldier,
});

#[derive(Debug, Clone, Deserialize, Default, Serialize)]
pub struct Crew(pub BTreeMap<CrewId, CrewMember>);
impl Crew {
    pub fn sum_wages(&self) -> f64 {
        self.0.values().map(|crew| crew.wage()).sum::<f64>()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CrewMember {
    pub member_type: CrewMemberType,
    pub rank: u8,
}

impl From<CrewMemberType> for CrewMember {
    fn from(member_type: CrewMemberType) -> Self {
        CrewMember {
            member_type,
            rank: 1,
        }
    }
}

impl CrewMember {
    pub fn wage(&self) -> f64 {
        let cfg = get_config();
        let op = cfg.crew.base_op_wage;
        let base = match self.member_type {
            CrewMemberType::Operator => op,
            CrewMemberType::Pilot => op * cfg.crew.pilot_wage_mult,
            CrewMemberType::Trader => op * cfg.crew.trader_wage_mult,
            CrewMemberType::Soldier => op * cfg.crew.solider_wage_mult,
        };
        base * (self.rank as f64).powf(cfg.crew.wage_rank_powf)
    }

    #[inline]
    pub fn price_next_rank(&self) -> f64 {
        let cfg = get_config();
        self.wage().powf(cfg.crew.wage_next_rank_powf) * cfg.crew.wage_next_rank_mult
    }
}
