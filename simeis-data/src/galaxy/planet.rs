use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hasher};

use rand::RngExt;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use crate::ship::resources::Resource;

use super::SpaceCoord;

pub type PlanetId = u64;

// Informations that can be scanned from a planet
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PlanetInfo {
    pub id: PlanetId,
    pub position: SpaceCoord,
    pub resources_density: BTreeMap<Resource, f64>,
}

impl PlanetInfo {
    pub fn scan(rank: u8, planet: &Planet) -> PlanetInfo {
        let mut resources_density = BTreeMap::new();
        for (res, density) in planet.resources_density.iter() {
            if res.min_rank() <= rank && density > &0.0 {
                resources_density.insert(*res, *density);
            }
        }
        PlanetInfo {
            id: planet.id,
            position: planet.position,
            resources_density,
        }
    }
}

#[derive(Debug)]
pub struct Planet {
    pub id: PlanetId,
    pub position: SpaceCoord,
    pub resources_density: BTreeMap<Resource, f64>,
}

impl Planet {
    pub fn random<R: rand::Rng>(coord: SpaceCoord, rng: &mut R) -> Planet {
        let mut resources_density = BTreeMap::new();
        for res in Resource::iter() {
            if res.pumpable(1) || res.mineable(1) || res.suckable(1) {
                resources_density.insert(res, rng.random_range(0.8..=1.0));
            } else if res.pumpable(u8::MAX) || res.mineable(u8::MAX) | res.suckable(u8::MAX) {
                resources_density.insert(res, rng.random_range(0.0..=1.0));
            }
        }
        Planet {
            id: id_from_coord(coord),
            position: coord,
            resources_density,
        }
    }
}

#[inline]
fn bell(x: f64, center: f64, width: f64) -> f64 {
    let d = (x - center) / width;
    (-d * d).exp()
}

#[inline]
fn inverse(x: f64) -> f64 {
    1.0 - x
}

#[inline]
pub fn calc_resource_density(
    resource: &Resource,
    temp: f64,
    density: f64,
    rot: f64,
    volc: f64,
    atm: f64,
) -> f64 {
    match resource {
        Resource::Iron => {
            0.3 * bell(temp, 0.5, 0.25)    // Moderate temperatures
            + 0.2 * density                // Denser rocky planets
            + 0.1 * bell(rot, 0.5, 0.30)   // Moderate rotation
            + 0.2 * bell(volc, 0.6, 0.25)  // Moderate / High volcanism
            + 0.2 * bell(atm, 0.6, 0.25) // Moderate / High pressure
        }

        Resource::Copper => {
            0.5 * density                  // High density
            + 0.3 * volc                   // High volcanism
            + 0.2 * inverse(atm) // Thin atmosphere
        }

        Resource::Titanium => {
            0.4 * bell(volc, 0.7, 0.20)    // High volcanism
            + 0.3 * density                // High density
            + 0.2 * bell(temp, 0.5, 0.25)  // Moderate temperature
            + 0.1 * bell(atm, 0.5, 0.30) // Moderate atmosphere
        }

        Resource::Uranium => {
            0.5 * bell(volc, 0.85, 0.15)  // High volcanism
            + 0.4 * density               // High density
            + 0.1 * inverse(rot) // Low rotation speed
        }

        Resource::Water => {
            0.4 * bell(temp, 0.45, 0.18)       // Moderate temperatures
            + 0.3 * bell(atm, 0.6, 0.25)       // Moderate pressure
            + 0.2 * bell(density, 0.5, 0.25)   // Moderate density
            + 0.1 * bell(rot, 0.5, 0.30) // Moderate rotation
        }

        Resource::Ammonia => {
            0.5 * bell(temp, 0.5, 0.15)       // Moderate temperatures
            + 0.3 * bell(atm, 0.6, 0.20)      // Moderate atmosphere pressure
            + 0.2 * bell(volc, 0.25, 0.25) // Low volcanic activity
        }

        Resource::SulfuricAcid => {
            0.4 * bell(temp, 0.3, 0.20)        // Low temperatures
            + 0.3 * bell(atm, 0.7, 0.20)       // High atmosphere pressure
            + 0.2 * inverse(volc)              // Low volcanism
            + 0.1 * bell(density, 0.6, 0.25) // Moderate / high density
        }

        Resource::Brine => {
            0.4 * bell(temp, 0.9, 0.12)    // High temperature
            + 0.3 * volc                   // High volcanism
            + 0.3 * bell(atm, 0.9, 0.15) // High athmosphere
        }

        Resource::Hydrogen => {
            0.5 * inverse(density)         // Low density
            + 0.3 * inverse(temp)          // Low temperature
            + 0.2 * bell(atm, 0.8, 0.20) // High atmosphere pressure
        }

        Resource::Oxygen => {
            0.4 * bell(temp, 0.5, 0.18)       // Moderate temperature
            + 0.3 * bell(atm, 0.65, 0.20)     // Moderate / high atmosphere pressure
            + 0.2 * bell(rot, 0.75, 0.30)     // High rotation speed
            + 0.1 * bell(density, 0.4, 0.25) // Moderate / low density
        }

        Resource::Helium => {
            0.6 * inverse(density)          // Low density
            + 0.3 * inverse(temp)           // Low temperature
            + 0.1 * bell(atm, 0.9, 0.15) // High atmosphere pressure
        }

        Resource::Ozone => {
            0.4 * bell(temp, 0.5, 0.18)      // Moderate temperature
            + 0.3 * bell(atm, 0.7, 0.20)     // High atmosphere pressure
            + 0.2 * bell(rot, 0.5, 0.30)     // Moderate rotation speed
            + 0.1 * bell(density, 0.5, 0.25) // Moderate density
        }

        // Not extractable
        Resource::Hull | Resource::Fuel => 0.0,
    }
    .clamp(0.0, 1.0)
}

#[inline]
fn id_from_coord(coord: SpaceCoord) -> PlanetId {
    let mut hasher = DefaultHasher::new();
    hasher.write_u32(coord.0);
    hasher.write_u32(coord.1);
    hasher.write_u32(coord.2);
    hasher.finish()
}
