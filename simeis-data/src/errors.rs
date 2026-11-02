#[derive(Debug)]
pub enum Errcode {
    NoPlayerKey,
    InvalidPlayerKey,
    PlayerNotFound(crate::player::PlayerId),
    PlayerAlreadyExists(String),
    NoPlayerWithKey,
    ShipNotFound(crate::ship::ShipId),
    NotEnoughMoney(f64, f64),
    InvalidArgument(&'static str),
    ShipNotExtracting,
    ShipNotIdle,
    CrewMemberNotIdle(crate::crew::CrewId),
    CrewMemberIdle(crate::crew::CrewId),
    CrewNotNeeded,
    CannotPerformTravel,
    NullDistance,
    NoSuchStation(crate::galaxy::station::StationId),
    NoSuchModule(crate::ship::module::ShipModuleId),
    AllModulesTaken,
    CannotExtractWithoutPlanet,
    CannotExtractWithoutModule,
    ShipNotInStation,
    WrongCrewType(crate::crew::CrewMemberType),
    CargoFull,
    NoTraderAssigned,
    NoPilotAssigned,
    BuyNothing,
    SellNothing,
    NoFuelInCargo,
    NoHullInCargo,
    CrewMemberNotFound(crate::crew::CrewId),
    PlayerLost,
    GameSignalSend,
    NoSuchIndustryUnit,
    NothingAtThisCoord,
    NotAPlanet,
    PlanetNotFound(crate::galaxy::planet::PlanetId),
    NotAnAdmin,
    InvalidAdminToken,
    MissingQueryArgument(&'static str),
    ReadConfigFile(std::io::Error),
    LoadConfigJson(serde_json::Error),
    DefaultConfigToJson(serde_json::Error),
    WriteDefaultConfig(std::io::Error),
    SerializeConfig(serde_json::Error),
}

impl Errcode {
    pub fn errmsg(&self) -> String {
        match self {
            Errcode::NoPlayerKey => "No player key provided with the request".to_string(),
            Errcode::InvalidPlayerKey => "The provided player key isn't valid".to_string(),
            Errcode::PlayerNotFound(id) => format!("No player was found with this ID: {id}"),
            Errcode::PlayerAlreadyExists(name) => format!("Player {name} already exists"),
            Errcode::NoPlayerWithKey => "No player with this key exists in this game".to_string(),
            Errcode::ShipNotFound(id) => format!("Ship of id {id} not found"),
            Errcode::NotEnoughMoney(got, need) => {
                format!("Not enough money, need {need}, got {got}")
            }
            Errcode::InvalidArgument(arg) => format!("Argument {arg} has an invalid value"),
            Errcode::CrewMemberNotIdle(id) => format!("Crew member {id} is already occupied"),
            Errcode::CrewNotNeeded => "This crew member is not needed aboard this ship".to_string(),
            Errcode::CannotPerformTravel => {
                "This travel cannot be done with the current state of the ship".to_string()
            }
            Errcode::NullDistance => "You already are on this coordinates".to_string(),
            Errcode::NoSuchStation(id) => format!("You don't own any station of id {id}"),
            Errcode::NoSuchModule(id) => format!("Ship module of id {id} doesn't exist"),
            Errcode::AllModulesTaken => "All modules already have an operator assigned".to_string(),
            Errcode::CannotExtractWithoutPlanet => {
                "Cannot extract resources, this ship is not on a planet".to_string()
            }
            Errcode::ShipNotInStation => "This ship is not docked on station".to_string(),
            Errcode::WrongCrewType(ctype) => {
                format!("This module requires a crew member of type {ctype:?}")
            }
            Errcode::CargoFull => "The cargo is full".to_string(),
            Errcode::ShipNotIdle => "The ship is already occupied with a task".to_string(),
            Errcode::ShipNotExtracting => "This ship is not extracting".to_string(),
            Errcode::NoTraderAssigned => "This station doesn't have a trader assigned".to_string(),
            Errcode::BuyNothing => "Either you attempted to BUY 0 units, or you don't have enough space in cargo to hold the resources".to_string(),
            Errcode::SellNothing => "Either you attempted to SELL 0 units, or you don't have any unit of this resource in your cargo".to_string(),
            Errcode::NoFuelInCargo => "You don't have any fuel in the station cargo".to_string(),
            Errcode::NoHullInCargo => "You don't have any hull in the station cargo".to_string(),
            Errcode::CrewMemberNotFound(id) => format!("Crew member of id {id} not found"),
            Errcode::PlayerLost => "This player lost the game and cannot play anymore".to_string(),
            Errcode::NoPilotAssigned => "No pilot is assigned on this ship".to_string(),
            Errcode::GameSignalSend => "Error while sending a game signal to state".to_string(),
            Errcode::CannotExtractWithoutModule => "Cannot extract because the ship doesn't have the required module".to_string(),
            Errcode::NoSuchIndustryUnit => "The provided industry unit doesn't exist".to_string(),
            Errcode::NothingAtThisCoord => "There is nothing at the specified space coordinates".to_string(),
            Errcode::NotAPlanet => "The specified space coordinates doesn't point to a planet".to_string(),
            Errcode::NotAnAdmin => "You are not an administrator".to_string(),
            Errcode::InvalidAdminToken => "The administrator token you provided is wrong".to_string(),
            Errcode::MissingQueryArgument(key) => format!("The query argument {key:?} is missing"),
            Errcode::PlanetNotFound(pid) => format!("The planet of ID {pid} doesn't exist"),
            Errcode::CrewMemberIdle(cid) => format!("Crew member {cid} is Idle"),
            Errcode::ReadConfigFile(error) => format!("Error while reading config file: {error:?}"),
            Errcode::LoadConfigJson(error) => format!("Error while loading config from JSON format: {error:?}"),
            Errcode::DefaultConfigToJson(error) => format!("Error while serializing default configuration to JSON: {error:?}"),
            Errcode::WriteDefaultConfig(error) => format!("Error while saving default config to file: {error:?}"),
            Errcode::SerializeConfig(error) => format!("Unable to serialize game configuration: {error:?}"),
        }
    }
}
