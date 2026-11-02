use mea::rwlock::RwLock;
use rand::{Rng, RngExt};
use rand_distr::{Distribution, Normal};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use strum::IntoEnumIterator;

use crate::config::get_config;
use crate::crew::CrewMember;
use crate::ship::resources::Resource;

#[inline]
pub fn fee_rate(rank: u8) -> f64 {
    let cfg = get_config();
    cfg.market.base_fee_rate / (rank as f64).powf(cfg.market.fee_rate_dev_powf)
}

pub struct Market {
    pub prices: BTreeMap<Resource, RwLock<f64>>,
}

impl Market {
    pub fn init() -> Market {
        let mut prices = BTreeMap::new();
        for r in Resource::iter() {
            prices.insert(r, RwLock::new(r.base_price()));
        }
        Market { prices }
    }

    pub async fn reset(&self) {
        for (r, p) in self.prices.iter() {
            *p.write().await = r.base_price();
        }
    }

    pub async fn to_json(&self) -> serde_json::Value {
        let mut resources = BTreeMap::new();
        for (res, price) in self.prices.iter() {
            let price = price.read().await;
            resources.insert(res, *price);
        }
        serde_json::to_value(resources).unwrap()
    }

    #[inline]
    fn rand_distrib(&self, r: &Resource, now_price: f64) -> Normal<f64> {
        let base_price = r.base_price();
        let pratio = now_price / base_price;
        // 0.3    AVG = 1 - 0.3 = 0.7  * MAX AMPL = 3.5 * 0.7  =  2.45
        // 1.3    AVG = 1 - 1.3 = -0.3 * MAX AMPL = 3.5 * -0.3 = -1.05
        let cfg = get_config();
        let avg = (1.0 - pratio) * cfg.market.max_avg_ampl;
        let std = avg.abs() + (cfg.market.max_avg_ampl / cfg.market.std_div);
        rand_distr::Normal::new(avg, std).unwrap()
    }

    #[inline]
    fn get_new_price<R: Rng>(&self, rng: &mut R, r: &Resource, old: f64) -> f64 {
        let distr = self.rand_distrib(r, old);
        let change = distr.sample(rng).clamp(-0.3, 0.3); // 30% change max in one tick

        old * (1.0 + change)
    }

    pub async fn update_prices<R: Rng>(&self, rng: &mut R) {
        let cfg = get_config();
        for (res, price) in self.prices.iter() {
            if !rng.random_bool(cfg.market.upd_price_proba) {
                continue;
            }
            let mut price = price.write().await;

            let new_price = self.get_new_price(rng, res, *price);
            log::trace!(
                "{res:?} {new_price} ({:?}%)",
                (new_price / res.base_price()) * 100.0
            );
            *price = new_price;
        }
    }

    pub async fn buy(&self, trader: &CrewMember, r: &Resource, amnt: f64) -> (MarketTx, f64) {
        assert!(amnt > 0.0);
        let fee_rate = fee_rate(trader.rank);

        let price = self.prices.get(r).unwrap();
        let mut price = price.write().await;
        assert!(*price > 0.0);
        let cost = amnt * *price;
        let fees = cost * fee_rate;
        let got_price = *price;

        let cfg = get_config();
        let rel_price = *price / r.base_price();
        let pratio = cfg.market.influence_buy(cost, rel_price);
        log::debug!("Influence BUY: {pratio}");
        *price *= pratio;
        assert!(*price > 0.0);

        (
            MarketTx {
                added_cargo: Some((*r, amnt)),
                removed_money: Some(cost + fees),
                fees,
                ..Default::default()
            },
            got_price,
        )
    }

    pub async fn sell(&self, trader: &CrewMember, r: &Resource, amnt: f64) -> (MarketTx, f64) {
        assert!(amnt > 0.0);
        let fee_rate = fee_rate(trader.rank);

        let price = self.prices.get(r).unwrap();
        let mut price = price.write().await;
        assert!(*price > 0.0);
        let cost = amnt * *price;
        let fees = cost * fee_rate;
        let got_price = *price;

        let cfg = get_config();
        let rel_price = *price / r.base_price();
        let pratio = cfg.market.influence_sell(cost, rel_price);
        log::debug!("Influence SELL: {pratio}");
        *price *= pratio;
        assert!(*price > 0.0);

        (
            MarketTx {
                removed_cargo: Some((*r, amnt)),
                added_money: Some(cost - fees),
                fees,
                ..Default::default()
            },
            got_price,
        )
    }
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct MarketTx {
    pub added_cargo: Option<(Resource, f64)>,
    pub removed_cargo: Option<(Resource, f64)>,

    pub added_money: Option<f64>,
    pub removed_money: Option<f64>,
    pub fees: f64,
}
