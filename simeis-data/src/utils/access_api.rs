// Utilities for API accesses
// Nothing interesting for the game here
// WHY:
//     No RwLock access must be dealt inside the web api code
//     Everything must be checked inside the Game struct
//     So I create useful interfaces to map to specific elements inside
//     It's verbose, but it works well and improves resiliency against deadlocks

use crate::errors::Errcode;
use crate::galaxy::planet::{Planet, PlanetId};
use crate::galaxy::station::{Station, StationId};
use crate::galaxy::{Galaxy, SpaceCoord, SpaceObject};
use crate::game::Game;
use crate::history::PlayerHistory;
use crate::player::{Player, PlayerId, PlayerKey};
use crate::ship::{Ship, ShipId};
use crate::utils::BoxFuture;

impl Game {
    #[inline]
    pub async fn map_galaxy_coord<F, T>(&self, coord: &SpaceCoord, f: F) -> Result<T, Errcode>
    where
        F: for<'a> FnOnce(&'a SpaceObject, &'a Galaxy) -> BoxFuture<'a, Result<T, Errcode>>,
    {
        let galaxy = self.galaxy.read().await;
        let obj = galaxy.get_object(coord);
        if obj.is_nothing() {
            return Err(Errcode::NothingAtThisCoord);
        };
        let data = f(obj, &galaxy).await;
        data
    }

    #[inline]
    pub async fn sync_map_planet_id<F, T>(&self, pid: &PlanetId, f: F) -> Result<T, Errcode>
    where
        F: for<'a> FnOnce(&'a Planet) -> Result<T, Errcode>,
    {
        let galaxy = self.galaxy.read().await;
        let Some(pla) = galaxy.planets.get(pid) else {
            return Err(Errcode::PlanetNotFound(*pid));
        };
        f(pla)
    }

    #[inline]
    pub async fn map_station<F, T>(
        &self,
        pkey: &PlayerKey,
        id: &StationId,
        f: F,
    ) -> Result<T, Errcode>
    where
        F: for<'a> FnOnce(
            PlayerId,
            &'a PlayerHistory,
            &'a Station,
        ) -> BoxFuture<'a, Result<T, Errcode>>,
    {
        let (pid, player) = self.get_player(pkey).await?;
        let player = player.read().await;
        let Some(station) = player.stations.get(id) else {
            return Err(Errcode::NoSuchStation(*id));
        };
        let data = f(pid, &player.history, station).await;
        data
    }

    #[inline]
    pub async fn map_ship<F, T>(&self, pkey: &PlayerKey, id: &ShipId, f: F) -> Result<T, Errcode>
    where
        F: for<'a> FnOnce(
            PlayerId,
            &'a PlayerHistory,
            &'a Ship,
        ) -> BoxFuture<'a, Result<T, Errcode>>,
    {
        let (pid, player) = self.get_player(pkey).await?;
        let player = player.read().await;
        let ship = player.get_ship(id)?;
        let data = f(pid, &player.history, ship).await;
        data
    }

    #[inline]
    pub async fn map_ship_mut<F, T>(
        &self,
        pkey: &PlayerKey,
        id: &ShipId,
        f: F,
    ) -> Result<T, Errcode>
    where
        F: for<'a> FnOnce(
            PlayerId,
            &'a PlayerHistory,
            &'a mut Ship,
        ) -> BoxFuture<'a, Result<T, Errcode>>,
    {
        let (pid, player) = self.get_player(pkey).await?;
        let mut player = player.write().await;
        let history = player.history.clone();
        let ship = player.get_ship_mut(id)?;
        let data = f(pid, &history, ship).await;
        data
    }

    #[inline]
    pub async fn map_ship_in_station<F, T>(
        &self,
        pkey: &PlayerKey,
        station_id: &StationId,
        ship_id: &ShipId,
        f: F,
    ) -> Result<T, Errcode>
    where
        F: for<'a> FnOnce(
            PlayerId,
            &'a PlayerHistory,
            &'a Station,
            &'a Ship,
        ) -> BoxFuture<'a, Result<T, Errcode>>,
    {
        let (pid, player) = self.get_player(pkey).await?;
        let player = player.read().await;
        if !player.ship_in_station(ship_id, station_id)? {
            return Err(Errcode::ShipNotInStation);
        }
        // SAFETY Checked in function above
        let ship = player.ships.get(ship_id).unwrap();
        let station = player.stations.get(station_id).unwrap();
        let data = f(pid, &player.history, station, ship).await;
        data
    }

    #[inline]
    pub async fn map_ship_mut_in_station<F, T>(
        &self,
        pkey: &PlayerKey,
        station_id: &StationId,
        ship_id: &ShipId,
        f: F,
    ) -> Result<T, Errcode>
    where
        F: for<'a> FnOnce(
            PlayerId,
            &'a PlayerHistory,
            &'a Station,
            &'a mut Ship,
        ) -> BoxFuture<'a, Result<T, Errcode>>,
    {
        let (pid, player) = self.get_player(pkey).await?;
        let mut player = player.write().await;
        if !player.ship_in_station(ship_id, station_id)? {
            return Err(Errcode::ShipNotInStation);
        }
        // SAFETY Checked in function above
        let station = player.stations.get(station_id).unwrap().clone();
        let history = player.history.clone();
        let ship = player.ships.get_mut(ship_id).unwrap();
        let data = f(pid, &history, &station, ship).await;
        data
    }

    #[inline]
    pub async fn sync_map_player_by_id<F, T>(&self, pid: &PlayerId, f: F) -> Result<T, Errcode>
    where
        F: for<'a> FnOnce(&'a Player, &'a PlayerHistory) -> Result<T, Errcode>,
    {
        let Some(player) = self.players.clone_val(pid).await else {
            return Err(Errcode::PlayerNotFound(*pid));
        };
        let player = player.read().await;
        f(&player, &player.history)
    }

    #[inline]
    pub async fn map_player_by_id<F, T>(&self, pid: &PlayerId, f: F) -> Result<T, Errcode>
    where
        F: for<'a> FnOnce(&'a Player) -> BoxFuture<'a, Result<T, Errcode>>,
    {
        let Some(player) = self.players.clone_val(pid).await else {
            return Err(Errcode::PlayerNotFound(*pid));
        };
        let player = player.read().await;
        let data = f(&player).await;
        data
    }

    #[inline]
    pub async fn map_player<F, T>(&self, pkey: &PlayerKey, f: F) -> Result<T, Errcode>
    where
        F: for<'a> FnOnce(&'a Player) -> BoxFuture<'a, Result<T, Errcode>>,
    {
        let (_, player) = self.get_player(pkey).await?;
        let player = player.read().await;
        let data = f(&player).await;
        data
    }

    #[inline]
    pub async fn map_player_mut<F, T>(&self, pkey: &PlayerKey, f: F) -> Result<T, Errcode>
    where
        F: for<'a> FnOnce(&'a mut Player) -> BoxFuture<'a, Result<T, Errcode>>,
    {
        let (_, player) = self.get_player(pkey).await?;
        let mut player = player.write().await;
        let data = f(&mut player).await;
        data
    }

    #[inline]
    pub async fn player_to_json(
        &self,
        pkey: &PlayerKey,
        id: &PlayerId,
    ) -> Result<serde_json::Value, Errcode> {
        let (pid, player) = self.get_player(pkey).await?;
        let player = player.read().await;
        if pid == *id {
            Ok(player.export_json(true))
        } else {
            self.export_player_data(id, false).await
        }
    }

    #[inline]
    pub async fn export_player_data(
        &self,
        id: &PlayerId,
        full: bool,
    ) -> Result<serde_json::Value, Errcode> {
        let Some(player) = self.players.clone_val(id).await else {
            return Err(Errcode::PlayerNotFound(*id));
        };
        let player = player.read().await;
        Ok(player.export_json(full))
    }

    #[inline]
    pub async fn remove_player(&self, id: &PlayerId) -> Result<(), Errcode> {
        let val = self
            .players
            .remove(id)
            .await
            .ok_or(Errcode::PlayerNotFound(*id))?;
        self.player_index.remove_value(id).await;
        self.taken_names.remove_value(id).await;
        let mut player = val.write().await;
        player.lost = true;
        Ok(())
    }
}
