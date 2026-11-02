use std::collections::BTreeMap;
use std::sync::Arc;

use rand::rngs::ThreadRng;
use rand::RngExt;

pub mod planet;
pub mod scan;
pub mod station;

use scan::ScanResult;
use station::StationId;

use crate::config::get_config;
use crate::galaxy::planet::{Planet, PlanetId};
use crate::galaxy::station::Station;

pub type SpaceUnit = u32;
pub type SpaceCoord = (SpaceUnit, SpaceUnit, SpaceUnit);
pub type SectorId = u32;
type GalaxySector = (
    (SpaceUnit, SpaceUnit),
    (SpaceUnit, SpaceUnit),
    (SpaceUnit, SpaceUnit),
);

#[derive(Debug)]
pub enum SpaceObject {
    Nothing,
    Station(StationId),
    Planet(PlanetId),
}

impl SpaceObject {
    pub fn is_nothing(&self) -> bool {
        matches!(self, SpaceObject::Nothing)
    }
}

// TODO (#31) Use a RwLock on each of the field, and remove the one from the Game struct
pub struct Galaxy {
    pub stations: BTreeMap<StationId, Arc<Station>>,
    pub planets: BTreeMap<PlanetId, Arc<Planet>>,
    objects: BTreeMap<SpaceCoord, SpaceObject>,
    discovered: BTreeMap<SectorId, GalaxySector>,
}

impl Galaxy {
    pub fn init() -> Galaxy {
        Galaxy {
            discovered: BTreeMap::new(),
            stations: BTreeMap::new(),
            planets: BTreeMap::new(),
            objects: BTreeMap::new(),
        }
    }

    // X, Y and Z can be any point from the given sector
    // Returns the index in the "discovered" vector
    pub fn generate_sector(&mut self, coord: &SpaceCoord) -> SectorId {
        let (x, y, z) = coord;
        let (secx, secy, secz) = compute_sector(*x, *y, *z);
        log::debug!(
            "Generating sector ({}-{}, {}-{}, {}-{})",
            secx.0,
            secx.1,
            secy.0,
            secy.1,
            secz.0,
            secz.1,
        );
        let id = self.discovered.len() as u32;
        self.discovered.insert(id, (secx, secy, secz));
        let mut rng = rand::rng();
        let mut npla = 0;
        let cfg = get_config();
        while npla < cfg.game.planets_per_sector {
            let x = rng.random_range(secx.0..secx.1);
            let y = rng.random_range(secy.0..secy.1);
            let z = rng.random_range(secz.0..secz.1);
            let planet = Planet::random((x, y, z), &mut rng);
            if let std::collections::btree_map::Entry::Vacant(e) =
                self.objects.entry(planet.position)
            {
                e.insert(SpaceObject::Planet(planet.id));
                self.planets.insert(planet.id, Arc::new(planet));
                npla += 1;
            }
        }
        id
    }

    pub fn is_discovered(&self, coord: &SpaceCoord) -> bool {
        let (x, y, z) = coord;
        for ((sx, ex), (sy, ey), (sz, ez)) in self.discovered.values() {
            if (x < sx) || (x > ex) || (y < sy) || (y > ey) || (z < sz) || (z > ez) {
                continue;
            }
            return true;
        }
        false
    }

    fn list_planets_in_sector(&self, sector: &GalaxySector) -> Vec<&Arc<Planet>> {
        let mut objects = vec![];
        for planet in self.planets.values() {
            let (x, y, z) = planet.position;
            if (x < sector.0 .0) || (x > sector.0 .1) {
                continue;
            }
            if (y < sector.1 .0) || (y > sector.1 .1) {
                continue;
            }
            if (z < sector.2 .0) || (z > sector.2 .1) {
                continue;
            }
            objects.push(planet);
        }
        objects
    }

    pub fn get_object(&self, coord: &SpaceCoord) -> &SpaceObject {
        self.objects.get(coord).unwrap_or(&SpaceObject::Nothing)
    }

    pub fn get_planet(&self, coord: &SpaceCoord) -> Option<Arc<Planet>> {
        let obj = self.get_object(coord);
        let SpaceObject::Planet(planet_id) = obj else {
            return None;
        };
        let planet = self.planets.get(planet_id).unwrap();
        Some(planet.clone())
    }

    // TODO (#31) Generate based on the galaxy
    pub async fn init_new_station(&mut self) -> (StationId, Arc<Station>) {
        let mut rng = rand::rng();

        let mut seccoord = (rng.random(), rng.random(), rng.random());
        while self.is_discovered(&seccoord) {
            seccoord = (rng.random(), rng.random(), rng.random());
        }
        let id = rng.random();
        let secid = self.generate_sector(&seccoord);
        let sector = self.discovered.get(&secid).unwrap();

        let all_planets = self.list_planets_in_sector(sector);
        assert!(!all_planets.is_empty());

        let mut coord;
        let mut retry_n = 0;
        let cfg = get_config();
        loop {
            let pla = all_planets[retry_n % all_planets.len()];
            coord = get_rand_coord_near(&pla.position, cfg.game.station_fplanet_dist, &mut rng);
            while !(is_in_sector(&coord, sector) && self.get_object(&coord).is_nothing()) {
                coord = get_rand_coord_near(&pla.position, cfg.game.station_fplanet_dist, &mut rng);
            }

            let mut mindist = None;
            for pla in self.list_planets_in_sector(sector) {
                let dist = get_distance(&pla.position, &coord);
                if let Some(ref mut m) = mindist {
                    if dist < *m {
                        *m = dist;
                    }
                } else {
                    mindist = Some(dist);
                }
            }

            let mindist = mindist.unwrap();
            if (mindist - cfg.game.station_fplanet_dist).abs() < 1.0 {
                break;
            }
            retry_n += 1;
            log::warn!("{retry_n} {mindist} {}", cfg.game.station_fplanet_dist);
            if retry_n > 10000 {
                panic!("Too many retries");
            }
        }
        let station = Arc::new(station::Station::init(id, coord));
        self.objects
            .insert(station.position, SpaceObject::Station(station.id));
        self.stations.insert(station.id, station.clone());
        (id, station)
    }

    pub fn scan_sector(&self, rank: u8, center: &SpaceCoord) -> ScanResult {
        let sector = compute_sector(center.0, center.1, center.2);
        let mut results = ScanResult::empty();
        for pla in self.planets.values() {
            if !is_in_sector(&pla.position, &sector) {
                continue;
            }
            results.add_planet(rank, pla);
        }

        for sta in self.stations.values() {
            if !is_in_sector(&sta.position, &sector) {
                continue;
            }
            results.add_station(rank, sta);
        }

        debug_assert!(!results.planets.is_empty()); // We should always have some planets
        results
    }
}

#[inline]
pub fn get_delta(a: &SpaceCoord, b: &SpaceCoord) -> (f64, f64, f64) {
    (
        (b.0 as f64) - (a.0 as f64),
        (b.1 as f64) - (a.1 as f64),
        (b.2 as f64) - (a.2 as f64),
    )
}

#[inline]
pub fn get_distance(a: &SpaceCoord, b: &SpaceCoord) -> f64 {
    let delta = get_delta(a, b);
    (delta.0.powf(2.0) + delta.1.powf(2.0) + delta.2.powf(2.0)).sqrt()
}

#[inline]
pub fn get_direction(a: &SpaceCoord, b: &SpaceCoord) -> (f64, f64, f64) {
    let delta = get_delta(a, b);
    let distance = get_distance(a, b);
    (delta.0 / distance, delta.1 / distance, delta.2 / distance)
}

// TODO (#13)   Unit tests on this one
fn compute_sector(x: SpaceUnit, y: SpaceUnit, z: SpaceUnit) -> GalaxySector {
    let cfg = get_config();
    let start_x = x - (x % cfg.game.sector_size);
    let end_x = start_x.saturating_add(cfg.game.sector_size);
    let start_y = y - (y % cfg.game.sector_size);
    let end_y = start_y.saturating_add(cfg.game.sector_size);
    let start_z = z - (z % cfg.game.sector_size);
    let end_z = start_z.saturating_add(cfg.game.sector_size);
    ((start_x, end_x), (start_y, end_y), (start_z, end_z))
}

pub fn translation(start: SpaceCoord, direction: (f64, f64, f64), dist: f64) -> SpaceCoord {
    (
        ((start.0 as f64) + (dist * direction.0)) as SpaceUnit,
        ((start.1 as f64) + (dist * direction.1)) as SpaceUnit,
        ((start.2 as f64) + (dist * direction.2)) as SpaceUnit,
    )
}

fn is_in_sector(coord: &SpaceCoord, sector: &GalaxySector) -> bool {
    coord.0 >= sector.0 .0
        && coord.0 < sector.0 .1
        && coord.1 >= sector.1 .0
        && coord.1 < sector.1 .1
        && coord.2 >= sector.2 .0
        && coord.2 < sector.2 .1
}

fn get_rand_coord_near(obj: &SpaceCoord, dist: f64, rng: &mut ThreadRng) -> SpaceCoord {
    let theta = rng.random_range(0.0..2.0 * std::f64::consts::PI); // azimuthal angle
    let phi = rng.random_range(0.0..std::f64::consts::PI); // polar angle
    let x = (obj.0 as f64) + (dist * phi.sin() * theta.cos());
    let y = (obj.1 as f64) + (dist * phi.sin() * theta.sin());
    let z = (obj.2 as f64) + (dist * phi.cos());
    (x as u32, y as u32, z as u32)
}

#[test]
fn test_compute_sector() {
    crate::config::init_default_config();
    let mut rng: rand::rngs::SmallRng = rand::make_rng();
    for _ in 0..10000000 {
        let x = rng.random();
        let y = rng.random();
        let z = rng.random();
        let sec = compute_sector(x, y, z);
        assert!(is_in_sector(&(x, y, z), &sec));
    }
    let cfg = get_config();
    let sector_size = cfg.game.sector_size;
    assert_eq!(
        compute_sector(sector_size - 1, 0, 0),
        ((0, sector_size), (0, sector_size), (0, sector_size))
    );
    assert_eq!(
        compute_sector(0, sector_size - 1, 0),
        ((0, sector_size), (0, sector_size), (0, sector_size))
    );
    assert_eq!(
        compute_sector(0, 0, sector_size - 1),
        ((0, sector_size), (0, sector_size), (0, sector_size))
    );

    assert_eq!(
        compute_sector(sector_size, 0, 0),
        (
            (sector_size, 2 * sector_size),
            (0, sector_size),
            (0, sector_size)
        )
    );
    assert_eq!(
        compute_sector(0, sector_size, 0),
        (
            (0, sector_size),
            (sector_size, 2 * sector_size),
            (0, sector_size)
        )
    );
    assert_eq!(
        compute_sector(0, 0, sector_size),
        (
            (0, sector_size),
            (0, sector_size),
            (sector_size, 2 * sector_size)
        )
    );
}
